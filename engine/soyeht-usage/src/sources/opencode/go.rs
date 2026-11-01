//! The OpenCode Go allowance: the five-hour, weekly and monthly share of the Go subscription.
//!
//! Source status: experimental. OpenCode's own core reads this for its usage dialog
//! (`UsageLimits` in `packages/core/src/usage-limits.ts`, anomalyco/opencode pull request 58694,
//! reached at `GET /api/experimental/usage/limits`). It asks Go's inference API
//! `GET {origin}/zen/go/v1/usage` with the Console login as a bearer token and the workspace of
//! that login in `x-opencode-org-id`. This is the same request with the same login, read-only; no
//! OpenCode server is started. The shape may change: anything that is not exactly the shape
//! below is "invalid response", never a number.

use super::{Credential, credential};
use crate::host::{Host, HttpRequest, HttpResult};
use crate::sources::{
    Attempt, Failure, ParsedReading, ReadContext, Source, http_body, iso_text, json, number,
    percent_window,
};
use serde_json::Value;
use soyeht_protocol::UsageSource;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_SERVER: &str = "https://opencode.ai/console";
const RETIRED_CONSOLE: &str = "https://console.opencode.ai";
const ROLLING_LABEL: &str = "5-hour";
const WEEKLY_LABEL: &str = "Weekly";

/// The OpenCode integration that holds the Console login. Both providers' models are billed
/// through it.
const INTEGRATION: &str = "opencode";

pub(super) struct GoAllowanceSource;

impl Source for GoAllowanceSource {
    fn id(&self) -> UsageSource {
        UsageSource::OpencodeGoAllowance
    }

    /// A missing or rejected login, or an account with no Go subscription, leaves the device
    /// activity to show; anything else is the answer.
    fn falls_back(&self, failure: &Failure) -> bool {
        matches!(
            failure,
            Failure::NotConfigured | Failure::LoginNeeded | Failure::NoAccess
        )
    }

    fn read(&self, host: &dyn Host, _context: ReadContext) -> Attempt {
        if !super::follows(host, &["opencode", "opencode-go"]) {
            return Err(Failure::NotConfigured);
        }
        let login = credential(host, INTEGRATION)?;
        let (url, org) = request_target(&login)?;
        let request = HttpRequest::get(url, TIMEOUT)
            .bearer(&login.secret)
            .header("x-opencode-org-id", &org)
            .header("Accept", "application/json");
        match host.http(&request) {
            // Go refuses (403) an account with no Go subscription: nothing to show, not an error.
            HttpResult::Response { status: 403, .. } => Err(Failure::NotConfigured),
            result => parse(&json(&http_body(result)?)?),
        }
    }
}

/// The usage URL and the workspace to ask it for. The Console named by the login may only be a
/// host of opencode.ai: the login travels to no other.
fn request_target(login: &Credential) -> Result<(String, String), Failure> {
    let org = login
        .org_id
        .clone()
        .filter(|org| !org.is_empty() && org.len() <= 128)
        .filter(|org| {
            org.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
        .ok_or(Failure::NotConfigured)?;
    let server = login.server.as_deref().unwrap_or(DEFAULT_SERVER);
    let server = match server.strip_prefix(RETIRED_CONSOLE) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            format!("{DEFAULT_SERVER}{rest}")
        }
        _ => server.to_owned(),
    };
    let host = server
        .strip_prefix("https://")
        .and_then(|rest| rest.split('/').next())
        .filter(|host| {
            (*host == "opencode.ai" || host.ends_with(".opencode.ai"))
                && host
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        })
        .ok_or(Failure::NotConfigured)?;
    Ok((format!("https://{host}/zen/go/v1/usage"), org))
}

/// `{"usage": {"rolling": M, "weekly": M, "monthly": M}}` where `M` is
/// `{"status": "ok" | "rate-limited", "percent": 0..100, "resetsAt": ISO 8601}`. The five-hour
/// meter is the primary window and the weekly one the secondary; the monthly meter has no slot
/// on the wire and is not shown.
pub(super) fn parse(document: &Value) -> Attempt {
    let usage = document.get("usage").ok_or(Failure::InvalidResponse)?;
    let meter = |name: &str, label: &str| {
        let meter = usage.get(name).ok_or(Failure::InvalidResponse)?;
        let percent = meter
            .get("percent")
            .and_then(number)
            .ok_or(Failure::InvalidResponse)?;
        let resets_at = meter
            .get("resetsAt")
            .and_then(Value::as_str)
            .and_then(iso_text);
        percent_window(percent, resets_at, label)
    };
    Ok(ParsedReading::windows(
        meter("rolling", ROLLING_LABEL)?,
        Some(meter("weekly", WEEKLY_LABEL)?),
    ))
}
