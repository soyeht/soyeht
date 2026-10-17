//! Claude: OAuth usage endpoint, then the Claude Code CLI's `/usage` screen, then the
//! claude.ai web session. This is CodexBar's app order in automatic mode
//! (`ClaudeSourcePlanner.makeSteps`, `.auto` + `.app`).

mod cli;
mod oauth;
mod screen;
mod web;

use super::{Failure, ParsedReading, Source, env_dir, iso_text, number, percent_window};
use crate::host::Host;
use serde_json::Value;
use std::path::PathBuf;

pub(crate) fn chain() -> Vec<Box<dyn Source>> {
    vec![
        Box::new(oauth::OAuthSource),
        Box::new(cli::CliSource),
        Box::new(web::WebSource),
    ]
}

/// Claude Code's configuration root: `$CLAUDE_CONFIG_DIR`, else `~/.claude`
/// (CodexBar `ClaudeConfigPaths`).
fn config_root(host: &dyn Host) -> Option<PathBuf> {
    env_dir(host, "CLAUDE_CONFIG_DIR").or_else(|| host.home().map(|home| home.join(".claude")))
}

const SESSION_LABEL: &str = "Current session";
const WEEKLY_LABEL: &str = "Weekly";

/// The windows of an OAuth or web usage response. Each window is
/// `{"utilization": <percent>, "resets_at": <ISO 8601>}` or `null`.
///
/// CodexBar's mapping: the primary window is the first present of the five-hour session, the
/// week, the OAuth-apps week, the Sonnet week and the Opus week; the secondary is the week.
fn parse_windows(document: &Value) -> Result<ParsedReading, Failure> {
    let object = document.as_object().ok_or(Failure::InvalidResponse)?;
    let candidates = [
        ("five_hour", SESSION_LABEL),
        ("seven_day", WEEKLY_LABEL),
        ("seven_day_oauth_apps", "Weekly (OAuth apps)"),
        ("seven_day_sonnet", "Weekly (Sonnet)"),
        ("seven_day_opus", "Weekly (Opus)"),
    ];
    let mut primary = None;
    for (key, label) in candidates {
        if let Some(window) = window(object.get(key), label)? {
            primary = Some((key, window));
            break;
        }
    }
    // Every window absent or null is a well-formed account with nothing reported yet, not a
    // malformed answer.
    let Some((primary_key, primary)) = primary else {
        return Err(Failure::NoData);
    };
    let secondary = if primary_key == "seven_day" {
        None
    } else {
        window(object.get("seven_day"), WEEKLY_LABEL)?
    };
    Ok(ParsedReading { primary, secondary })
}

fn window(value: Option<&Value>, label: &str) -> Result<Option<crate::Window>, Failure> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let Some(utilization) = value.get("utilization").filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let percent = number(utilization).ok_or(Failure::InvalidResponse)?;
    let resets_at = value
        .get("resets_at")
        .and_then(Value::as_str)
        .and_then(iso_text);
    percent_window(percent, resets_at, label).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn session_is_primary_and_week_secondary() {
        let reading = parse_windows(&json!({
            "five_hour": {"utilization": 12.5, "resets_at": "2026-10-03T18:00:00.123+00:00"},
            "seven_day": {"utilization": 40, "resets_at": "2026-10-07T12:00:00Z"},
            "seven_day_opus": null
        }))
        .unwrap();
        assert_eq!(reading.primary.used, 0.125);
        assert_eq!(reading.primary.label, SESSION_LABEL);
        assert_eq!(reading.primary.resets_at, "2026-10-03T18:00:00Z");
        assert_eq!(reading.secondary.unwrap().used, 0.4);
    }

    #[test]
    fn missing_session_promotes_the_week_without_repeating_it() {
        let reading = parse_windows(&json!({
            "five_hour": null,
            "seven_day": {"utilization": 70, "resets_at": null}
        }))
        .unwrap();
        assert_eq!(reading.primary.label, WEEKLY_LABEL);
        assert_eq!(reading.primary.resets_at, "");
        assert!(reading.secondary.is_none());
    }

    #[test]
    fn absent_or_empty_windows_are_no_data_not_an_invalid_response() {
        for document in [
            json!({}),
            json!({"five_hour": null, "seven_day": null}),
            json!({"five_hour": {}, "seven_day": {"utilization": null, "resets_at": null}}),
            json!({"extra_usage": {"is_enabled": false, "utilization": null}}),
        ] {
            assert_eq!(parse_windows(&document), Err(Failure::NoData), "{document}");
        }
    }

    #[test]
    fn malformed_windows_stay_an_invalid_response() {
        for document in [
            json!([]),
            json!("text"),
            json!({"five_hour": {"utilization": "high"}}),
        ] {
            assert_eq!(
                parse_windows(&document),
                Err(Failure::InvalidResponse),
                "{document}"
            );
        }
    }
}
