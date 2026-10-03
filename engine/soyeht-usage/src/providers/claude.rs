use super::{ParsedReading, ProviderReader};
use crate::{Error, Result, Window};
use chrono::{SecondsFormat, TimeZone, Utc};
use serde_json::{Map, Value};
use soyeht_protocol::{UsageProvider, UsageSource};

const SOURCES: &[UsageSource] = &[UsageSource::ClaudeDesktopCache];
/// The windows Claude Code caches, in display order. Each key is always present in a cached
/// reading; its value is `null` while the account has no such limit or the window has not
/// started, which is a missing window rather than malformed data.
const WINDOWS: [(&str, &str); 2] = [("five_hour", "Current session"), ("seven_day", "Weekly")];

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
    let invalid = || Error::InvalidResponse {
        source: source.clone(),
    };
    let document = super::json(payload, source.clone())?;
    // Only an object without the key is an unpopulated cache. Any other
    // top-level value (null, array, number) is malformed data.
    let object = document.as_object().ok_or_else(invalid)?;
    let cached = object
        .get("cachedUsageUtilization")
        .ok_or_else(|| Error::CacheNotPopulated {
            source: source.clone(),
        })?;
    let fetched_at_ms = cached["fetchedAtMs"]
        .as_u64()
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(invalid)?;
    let fetched_at = Utc
        .timestamp_millis_opt(fetched_at_ms)
        .single()
        .ok_or_else(invalid)?
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let utilization = cached
        .get("utilization")
        .and_then(Value::as_object)
        .ok_or_else(invalid)?;
    let mut windows = Vec::with_capacity(WINDOWS.len());
    for (key, label) in WINDOWS {
        if let Some(window) = parse_window(utilization, key, label, source.clone())? {
            windows.push(window);
        }
    }
    let mut windows = windows.into_iter();
    // A cache whose every window is null holds no reading yet.
    let primary = windows.next().ok_or_else(|| Error::CacheNotPopulated {
        source: source.clone(),
    })?;
    let observed_at_sec = u64::try_from(fetched_at_ms / 1_000).map_err(|_| invalid())?;
    Ok(ParsedReading {
        primary,
        secondary: windows.next(),
        fetched_at: Some(fetched_at),
        observed_at_sec: Some(observed_at_sec),
    })
}

/// A missing key is a partial cache and therefore invalid. A `null` window, or one whose
/// percentage or reset time is `null`, is a window Claude Code reports as not running: it is
/// left out instead of discarding the windows that do have a reading.
fn parse_window(
    utilization: &Map<String, Value>,
    key: &str,
    label: &str,
    source: UsageSource,
) -> Result<Option<Window>> {
    let invalid = || Error::InvalidResponse {
        source: source.clone(),
    };
    let value = utilization.get(key).ok_or_else(invalid)?;
    if value.is_null() {
        return Ok(None);
    }
    let window = value.as_object().ok_or_else(invalid)?;
    let percent = window.get("utilization").ok_or_else(invalid)?;
    let reset = window.get("resets_at").ok_or_else(invalid)?;
    if percent.is_null() || reset.is_null() {
        return Ok(None);
    }
    let percent = percent.as_f64().ok_or_else(invalid)?;
    let reset = reset.as_str().ok_or_else(invalid)?;
    Window::checked(
        super::percent_fraction(percent, source.clone())?,
        reset,
        label,
        source,
    )
    .map(Some)
}
