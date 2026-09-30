use super::{ParsedReading, ProviderReader};
use crate::{Error, Result, Window};
use serde_json::Value;
use soyeht_protocol::{UsageProvider, UsageSource};

const SOURCES: &[UsageSource] = &[
    UsageSource::CodexAppServer,
    UsageSource::CodexAccountEndpoint,
];

pub(crate) struct Reader;

impl ProviderReader for Reader {
    fn provider(&self) -> UsageProvider {
        UsageProvider::Codex
    }

    fn sources(&self) -> &'static [UsageSource] {
        SOURCES
    }

    fn parse(&self, source: UsageSource, payload: &[u8]) -> Result<ParsedReading> {
        if source != UsageSource::CodexAppServer {
            return Err(Error::UnverifiedResponse { source });
        }
        parse_app_server(payload, source)
    }
}

fn parse_app_server(payload: &[u8], source: UsageSource) -> Result<ParsedReading> {
    let document = super::json(payload, source.clone())?;
    let result = document.get("result").unwrap_or(&document);
    let limits = result
        .get("rateLimits")
        .ok_or_else(|| Error::InvalidResponse {
            source: source.clone(),
        })?;
    let primary = parse_window(
        &limits["primary"],
        window_label(&limits["primary"], "Current window"),
        source.clone(),
    )?;
    let secondary = limits
        .get("secondary")
        .filter(|value| !value.is_null())
        .map(|value| {
            parse_window(
                value,
                window_label(value, "Secondary window"),
                source.clone(),
            )
        })
        .transpose()?;
    Ok(ParsedReading {
        primary,
        secondary,
        fetched_at: None,
        observed_at_sec: None,
    })
}

fn window_label(value: &Value, fallback: &'static str) -> &'static str {
    match value["windowDurationMins"].as_u64() {
        Some(10_080) => "Weekly",
        _ => fallback,
    }
}

fn parse_window(value: &Value, label: &str, source: UsageSource) -> Result<Window> {
    let percent = value["usedPercent"]
        .as_f64()
        .ok_or_else(|| Error::InvalidResponse {
            source: source.clone(),
        })?;
    let reset = value["resetsAt"]
        .as_i64()
        .map(|value| value.to_string())
        .or_else(|| value["resetsAt"].as_str().map(str::to_owned))
        .ok_or_else(|| Error::InvalidResponse {
            source: source.clone(),
        })?;
    Window::checked(
        super::percent_fraction(percent, source.clone())?,
        reset,
        label,
        source,
    )
}
