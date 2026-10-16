//! Codex: a personal access token, then the ChatGPT login in `auth.json`, both sent to the
//! ChatGPT usage endpoint, then a disposable `codex app-server`. This is CodexBar's automatic
//! order (`CodexProviderDescriptor.resolveStrategies`: `[pat, oauth, cli]`).

mod app_server;
mod token;

use super::{Failure, ParsedReading, Source, env_dir, epoch_value, number, percent_window};
use crate::{Window, host::Host};
use serde_json::Value;
use std::path::PathBuf;

pub(crate) fn chain() -> Vec<Box<dyn Source>> {
    vec![
        Box::new(token::AccessTokenSource),
        Box::new(token::AccountSource),
        Box::new(app_server::AppServerSource),
    ]
}

/// `$CODEX_HOME`, else `~/.codex`.
fn codex_home(host: &dyn Host) -> Option<PathBuf> {
    env_dir(host, "CODEX_HOME").or_else(|| host.home().map(|home| home.join(".codex")))
}

const SESSION_MINUTES: u64 = 300;
const WEEK_MINUTES: u64 = 10_080;

fn label(minutes: Option<u64>, fallback: &'static str) -> &'static str {
    match minutes {
        Some(SESSION_MINUTES) => "5-hour window",
        Some(WEEK_MINUTES) => "Weekly",
        _ => fallback,
    }
}

/// One window with its length in minutes, before the session/week placement.
struct Lane {
    minutes: Option<u64>,
    window: Window,
}

/// CodexBar places windows by length: the five-hour window is primary and the week secondary,
/// whatever order the response used.
fn place(first: Option<Lane>, second: Option<Lane>) -> Result<ParsedReading, Failure> {
    let (first, second) = match (first, second) {
        (Some(first), Some(second))
            if first.minutes == Some(WEEK_MINUTES) && second.minutes != Some(WEEK_MINUTES) =>
        {
            (Some(second), Some(first))
        }
        (None, second) => (second, None),
        pair => pair,
    };
    let primary = first.ok_or(Failure::InvalidResponse)?.window;
    Ok(ParsedReading {
        primary,
        secondary: second.map(|lane| lane.window),
    })
}

/// The field names of one response shape.
struct Shape {
    container: &'static str,
    windows: [&'static str; 2],
    percent: &'static str,
    reset: &'static str,
    /// The window length in minutes.
    minutes: fn(&Value) -> Option<u64>,
}

/// The ChatGPT usage response: `rate_limit.primary_window` and `secondary_window`, each
/// `{used_percent, reset_at (Unix seconds), limit_window_seconds}`.
const USAGE_SHAPE: Shape = Shape {
    container: "rate_limit",
    windows: ["primary_window", "secondary_window"],
    percent: "used_percent",
    reset: "reset_at",
    minutes: |value| {
        value["limit_window_seconds"]
            .as_u64()
            .map(|seconds| seconds / 60)
    },
};

/// The app server's `account/rateLimits/read` result: `rateLimits.primary` and `secondary`,
/// each `{usedPercent, windowDurationMins, resetsAt (Unix seconds, a number or a string)}`.
const APP_SERVER_SHAPE: Shape = Shape {
    container: "rateLimits",
    windows: ["primary", "secondary"],
    percent: "usedPercent",
    reset: "resetsAt",
    minutes: |value| value["windowDurationMins"].as_u64(),
};

fn parse_shape(document: &Value, shape: &Shape) -> Result<ParsedReading, Failure> {
    let limits = document
        .get(shape.container)
        .filter(|value| value.is_object())
        .ok_or(Failure::InvalidResponse)?;
    let lane = |key: &str, fallback: &'static str| -> Result<Option<Lane>, Failure> {
        let Some(value) = limits.get(key).filter(|value| value.is_object()) else {
            return Ok(None);
        };
        let percent = number(&value[shape.percent]).ok_or(Failure::InvalidResponse)?;
        let minutes = (shape.minutes)(value);
        let resets_at = epoch_value(&value[shape.reset]);
        Ok(Some(Lane {
            minutes,
            window: percent_window(percent, resets_at, label(minutes, fallback))?,
        }))
    };
    place(
        lane(shape.windows[0], "Current window")?,
        lane(shape.windows[1], "Secondary window")?,
    )
}

fn parse_usage(document: &Value) -> Result<ParsedReading, Failure> {
    parse_shape(document, &USAGE_SHAPE)
}

fn parse_rate_limits(result: &Value) -> Result<ParsedReading, Failure> {
    parse_shape(result, &APP_SERVER_SHAPE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn usage_windows_are_placed_by_length() {
        let reading = parse_usage(&json!({"rate_limit": {
            "primary_window": {"used_percent": 61, "reset_at": 1800000000, "limit_window_seconds": 604800},
            "secondary_window": {"used_percent": 7, "reset_at": 1790000000, "limit_window_seconds": 18000}
        }}))
        .unwrap();
        assert_eq!(reading.primary.label, "5-hour window");
        assert_eq!(reading.primary.used, 0.07);
        let weekly = reading.secondary.unwrap();
        assert_eq!(weekly.label, "Weekly");
        assert_eq!(weekly.resets_at, "2027-01-15T08:00:00Z");
    }

    #[test]
    fn a_reset_time_is_unix_seconds_as_a_number_or_a_string() {
        let window = |reset: Value| {
            let reading = parse_rate_limits(&json!({"rateLimits": {"primary": {
                "usedPercent": 10, "windowDurationMins": 10080, "resetsAt": reset
            }}}))
            .unwrap();
            reading.primary.resets_at
        };
        assert_eq!(window(json!(1800000000)), "2027-01-15T08:00:00Z");
        assert_eq!(window(json!("1800000000")), "2027-01-15T08:00:00Z");
        assert_eq!(window(json!(" 1800000000 ")), "2027-01-15T08:00:00Z");
        for missing in [
            json!(null),
            json!(""),
            json!("soon"),
            json!("-5"),
            json!(-5),
            json!(1.5),
        ] {
            assert_eq!(window(missing.clone()), "", "{missing}");
        }
    }

    #[test]
    fn app_server_shape_reads_the_same_windows() {
        let reading = parse_rate_limits(&json!({"rateLimits": {
            "primary": {"usedPercent": 12.5, "windowDurationMins": 300, "resetsAt": 1790000000},
            "secondary": {"usedPercent": 40, "windowDurationMins": 10080, "resetsAt": 1800000000}
        }}))
        .unwrap();
        assert_eq!(reading.primary.used, 0.125);
        assert_eq!(reading.secondary.unwrap().label, "Weekly");
        assert_eq!(
            parse_rate_limits(&json!({"rateLimits": {}})),
            Err(Failure::InvalidResponse)
        );
    }
}
