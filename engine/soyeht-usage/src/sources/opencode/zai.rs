//! The GLM Coding Plan quota of Z.ai (and of Zhipu's BigModel, its mainland host), for an
//! OpenCode set to the `zai-coding-plan` or `zhipuai-coding-plan` provider.
//!
//! Source status: vendor plugin, no documented API. The request is the one Z.ai's own plugin for
//! its coding plan makes (github.com/zai-org/zai-coding-plugins,
//! `plugins/glm-plan-usage/skills/usage-query-skill/scripts/query-usage.mjs`):
//! `GET {host}/api/monitor/usage/quota/limit` with the plan's token in `Authorization`. The limit
//! entries are read the way CodexBar reads them (`Sources/CodexBarCore/Resources/Plugins/zai.js`,
//! MIT, © Peter Steinberger). The token is the one OpenCode stored for the provider.
//!
//! An account with no active plan answers HTTP 200 with
//! `{"code": 500, "msg": "...coding plan", "success": false}`; that is "plan expired".

use super::credential;
use crate::Window;
use crate::host::{Host, HttpRequest};
use crate::sources::{Attempt, Failure, ParsedReading, ReadContext, Source};
use crate::sources::{epoch_text, http_body, json, percent_window};
use serde_json::Value;
use soyeht_protocol::UsageSource;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(15);
const QUOTA_PATH: &str = "/api/monitor/usage/quota/limit";
/// OpenCode provider ids and the host of each plan.
const HOSTS: [(&str, &str); 2] = [
    ("zai-coding-plan", "https://api.z.ai"),
    ("zhipuai-coding-plan", "https://open.bigmodel.cn"),
];
/// A five-hour window cannot reset later than that (plus a minute of clock skew); a reset beyond
/// it is dropped and the percentage kept, as CodexBar does.
const FIVE_HOUR_MINUTES: i64 = 300;
const FIVE_HOUR_RESET_LIMIT_SEC: i64 = 5 * 3600 + 60;

pub(super) struct ZaiPlanSource;

impl Source for ZaiPlanSource {
    fn id(&self) -> UsageSource {
        UsageSource::OpencodeZaiPlan
    }

    fn read(&self, host: &dyn Host, context: ReadContext) -> Attempt {
        let Some(provider) = super::active_provider(host) else {
            return Err(Failure::NotConfigured);
        };
        let Some((_, base)) = HOSTS.iter().find(|(id, _)| *id == provider) else {
            return Err(Failure::NotConfigured);
        };
        // Set to a plan but not signed in to it: the person is to sign in.
        let login = credential(host, &provider).map_err(|failure| match failure {
            Failure::NotConfigured => Failure::LoginNeeded,
            other => other,
        })?;
        let request = HttpRequest::get(format!("{base}{QUOTA_PATH}"), TIMEOUT)
            .secret_header("Authorization", login.secret)
            .header("Accept-Language", "en-US,en")
            .header("Content-Type", "application/json");
        parse(&json(&http_body(host.http(&request))?)?, context.now_sec)
    }

    /// The plan is set and answered (or was refused): the device activity of the same provider
    /// would only hide an expired plan, so the chain ends here.
    fn falls_back(&self, failure: &Failure) -> bool {
        matches!(failure, Failure::NotConfigured)
    }
}

/// One `TOKENS_LIMIT` or `CREDIT_LIMIT` entry, with the length of its window.
struct Limit {
    window_minutes: Option<i64>,
    used_percent: f64,
    resets_at_ms: Option<i64>,
}

pub(super) fn parse(document: &Value, now_sec: u64) -> Attempt {
    if document["success"] != Value::Bool(true) || document["code"] != 200 {
        return Err(if no_plan(document) {
            Failure::PlanExpired
        } else {
            Failure::Failed
        });
    }
    let entries = document["data"]["limits"]
        .as_array()
        .ok_or(Failure::InvalidResponse)?;
    let mut limits = Vec::new();
    for entry in entries {
        match entry["type"].as_str() {
            Some("TOKENS_LIMIT" | "CREDIT_LIMIT") => limits.push(limit(entry)?),
            // The MCP lane (`TIME_LIMIT`) and any limit kind this reader does not know are not
            // the plan's window.
            Some(_) => {}
            None => return Err(Failure::InvalidResponse),
        }
    }
    // Shortest window first; a window of unknown length last.
    limits.sort_by_key(|limit| limit.window_minutes.unwrap_or(i64::MAX));
    let (Some(first), last) = (limits.first(), limits.last().filter(|_| limits.len() > 1)) else {
        return Err(Failure::NoData);
    };
    Ok(ParsedReading::windows(
        window(first, now_sec)?,
        last.map(|limit| window(limit, now_sec)).transpose()?,
    ))
}

/// The "no coding plan" answer. Its message is in the plan's language; the fragment that is
/// stable across languages is the product name.
fn no_plan(document: &Value) -> bool {
    document["code"] == 500
        && document["msg"]
            .as_str()
            .is_some_and(|message| message.to_ascii_lowercase().contains("coding plan"))
}

fn limit(entry: &Value) -> Result<Limit, Failure> {
    let integer = |name: &str| entry[name].as_i64();
    let (unit, number) = (integer("unit"), integer("number"));
    let percentage = entry["percentage"]
        .as_f64()
        .ok_or(Failure::InvalidResponse)?;
    if unit.is_none() || number.is_none() {
        return Err(Failure::InvalidResponse);
    }
    let mut used_percent = percentage;
    // A positive limit with its remaining (or current) count is the more exact figure.
    if let Some(total) = integer("usage").filter(|total| *total > 0) {
        let used = match (integer("remaining"), integer("currentValue")) {
            (Some(remaining), current) => Some((total - remaining).max(current.unwrap_or(0))),
            (None, current) => current,
        };
        if let Some(used) = used {
            used_percent = used.clamp(0, total) as f64 * 100.0 / total as f64;
        }
    }
    let minutes_per_unit = match unit {
        Some(1) => Some(1440),
        Some(3) => Some(60),
        Some(5) => Some(1),
        Some(6) => Some(10_080),
        _ => None,
    };
    Ok(Limit {
        window_minutes: number
            .filter(|count| *count > 0)
            .zip(minutes_per_unit)
            .map(|(count, per)| count.saturating_mul(per)),
        used_percent,
        resets_at_ms: integer("nextResetTime"),
    })
}

fn window(limit: &Limit, now_sec: u64) -> Result<Window, Failure> {
    let now = i64::try_from(now_sec).unwrap_or(i64::MAX);
    let resets_at = limit
        .resets_at_ms
        .map(|ms| ms.div_euclid(1000))
        .filter(|reset| {
            limit.window_minutes != Some(FIVE_HOUR_MINUTES)
                || *reset <= now.saturating_add(FIVE_HOUR_RESET_LIMIT_SEC)
        })
        .and_then(epoch_text);
    percent_window(
        limit.used_percent.clamp(0.0, 100.0),
        resets_at,
        &label(limit.window_minutes),
    )
}

/// "5-hour", "Weekly", "3-day", "90-minute"; a window of unknown length is the plan's.
fn label(minutes: Option<i64>) -> String {
    match minutes {
        None => "Plan".to_owned(),
        Some(10_080) => "Weekly".to_owned(),
        Some(minutes) if minutes % 1440 == 0 => format!("{}-day", minutes / 1440),
        Some(minutes) if minutes % 60 == 0 => format!("{}-hour", minutes / 60),
        Some(minutes) => format!("{minutes}-minute"),
    }
}
