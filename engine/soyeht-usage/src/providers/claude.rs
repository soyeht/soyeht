use super::{ParsedReading, ProviderReader};
use crate::{Error, Result, Window};
use chrono::{SecondsFormat, TimeZone, Utc};
use serde_json::Value;
use soyeht_protocol::{UsageProvider, UsageSource};

const SOURCES: &[UsageSource] = &[UsageSource::ClaudeDesktopCache];

pub(crate) struct Reader;

impl ProviderReader for Reader {
    fn provider(&self) -> UsageProvider {
        UsageProvider::Claude
    }

    fn sources(&self) -> &'static [UsageSource] {
        SOURCES
    }

    fn parse(&self, source: UsageSource, payload: &[u8]) -> Result<ParsedReading> {
        if source != UsageSource::ClaudeDesktopCache {
            return Err(Error::UnverifiedResponse { source });
        }
        parse_cache(payload, source)
    }
}

fn parse_cache(payload: &[u8], source: UsageSource) -> Result<ParsedReading> {
    let document = super::json(payload, source.clone())?;
    let cached = document
        .get("cachedUsageUtilization")
        .ok_or_else(|| Error::InvalidResponse {
            source: source.clone(),
        })?;
    let fetched_at_ms = cached["fetchedAtMs"]
        .as_u64()
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(|| Error::InvalidResponse {
            source: source.clone(),
        })?;
    let fetched_at = Utc
        .timestamp_millis_opt(fetched_at_ms)
        .single()
        .ok_or_else(|| Error::InvalidResponse {
            source: source.clone(),
        })?
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let utilization = cached
        .get("utilization")
        .ok_or_else(|| Error::InvalidResponse {
            source: source.clone(),
        })?;
    let primary = parse_window(&utilization["five_hour"], "Current session", source.clone())?;
    let secondary = parse_window(&utilization["seven_day"], "Weekly", source.clone())?;
    let observed_at_sec =
        u64::try_from(fetched_at_ms / 1_000).map_err(|_| Error::InvalidResponse {
            source: source.clone(),
        })?;
    Ok(ParsedReading {
        primary,
        secondary: Some(secondary),
        fetched_at: Some(fetched_at),
        observed_at_sec: Some(observed_at_sec),
    })
}

fn parse_window(value: &Value, label: &str, source: UsageSource) -> Result<Window> {
    let percent = value["utilization"]
        .as_f64()
        .ok_or_else(|| Error::InvalidResponse {
            source: source.clone(),
        })?;
    let reset = value["resets_at"]
        .as_str()
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
