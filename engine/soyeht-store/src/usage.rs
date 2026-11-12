//! Named persistence operations for normalized subscription-usage readings.

use crate::database::{Change, Error, Result, Store};
use rusqlite::{OptionalExtension, params};
use soyeht_protocol::{
    UsageActivity, UsageProvider, UsageReading, UsageSource, UsageState, UsageWindow,
};

#[derive(Clone, Debug, PartialEq)]
pub struct StoredUsageReading {
    pub reading: UsageReading,
    pub observed_at_unix_sec: i64,
}

impl Store {
    pub fn usage_reading(&self, provider: &UsageProvider) -> Result<Option<StoredUsageReading>> {
        let raw = self
            .connection
            .query_row(
                "SELECT observed_at_unix_sec,payload FROM usage_readings WHERE provider=?1",
                [provider_name(provider)?],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        raw.map(|(observed_at_unix_sec, payload)| {
            decode_stored(provider, observed_at_unix_sec, &payload)
        })
        .transpose()
    }

    pub fn usage_readings(&self) -> Result<Vec<StoredUsageReading>> {
        let mut statement = self
            .connection
            .prepare("SELECT provider,observed_at_unix_sec,payload FROM usage_readings")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut readings = Vec::new();
        for row in rows {
            let (provider, observed_at_unix_sec, payload) = row?;
            let expected = provider_from_name(&provider)?;
            readings.push(decode_stored(&expected, observed_at_unix_sec, &payload)?);
        }
        readings.sort_by_key(|stored| provider_rank(&stored.reading.provider));
        Ok(readings)
    }
}

impl Change<'_> {
    pub fn save_usage_reading(
        &self,
        reading: &UsageReading,
        observed_at_unix_sec: i64,
    ) -> Result<()> {
        validate(reading, observed_at_unix_sec)?;
        self.transaction.execute(
            "INSERT INTO usage_readings(provider,observed_at_unix_sec,payload) VALUES (?1,?2,?3) ON CONFLICT(provider) DO UPDATE SET observed_at_unix_sec=excluded.observed_at_unix_sec,payload=excluded.payload",
            params![
                provider_name(&reading.provider)?,
                observed_at_unix_sec,
                serde_json::to_string(reading)?,
            ],
        )?;
        Ok(())
    }
}

fn decode_stored(
    expected: &UsageProvider,
    observed_at_unix_sec: i64,
    payload: &str,
) -> Result<StoredUsageReading> {
    let reading: UsageReading = serde_json::from_str(payload)?;
    validate(&reading, observed_at_unix_sec)?;
    if &reading.provider != expected {
        return Err(Error::InvalidUsageReading);
    }
    Ok(StoredUsageReading {
        reading,
        observed_at_unix_sec,
    })
}

fn validate(reading: &UsageReading, observed_at_unix_sec: i64) -> Result<()> {
    let source_matches = match reading.provider {
        UsageProvider::Claude => matches!(
            reading.source,
            UsageSource::ClaudeCliUsage
                | UsageSource::ClaudeOauthEndpoint
                | UsageSource::ClaudeWeb
                | UsageSource::ClaudeDesktopCache
        ),
        UsageProvider::Codex => matches!(
            reading.source,
            UsageSource::CodexAppServer
                | UsageSource::CodexAccountEndpoint
                | UsageSource::CodexAccessToken
        ),
        UsageProvider::Cursor => matches!(
            reading.source,
            UsageSource::CursorAppLogin
                | UsageSource::CursorBrowserCookies
                | UsageSource::CursorCurrentPeriod
        ),
        UsageProvider::Opencode => matches!(
            reading.source,
            UsageSource::OpencodeGoAllowance
                | UsageSource::OpencodeZaiPlan
                | UsageSource::OpencodeLocalHistory
        ),
    };
    let state_matches = match reading.state {
        UsageState::Ok | UsageState::Stale => {
            reading.primary.is_some() || reading.activity.is_some()
        }
        UsageState::LoginNeeded | UsageState::NoAccess | UsageState::Unavailable => true,
        // Only a later engine writes a state this one does not know.
        UsageState::Unknown => false,
    };
    let windows_match = reading
        .primary
        .iter()
        .chain(&reading.secondary)
        .all(valid_window);
    if observed_at_unix_sec < 0
        || !reading.activity.iter().all(valid_activity)
        || reading.fetched_at.len() > 128
        || !source_matches
        || !state_matches
        || !windows_match
    {
        return Err(Error::InvalidUsageReading);
    }
    Ok(())
}

fn valid_activity(activity: &UsageActivity) -> bool {
    activity.tokens >= 0
        && activity
            .cost_usd
            .is_none_or(|cost| cost.is_finite() && cost >= 0.0)
        && !activity.label.is_empty()
        && activity.label.len() <= 128
}

fn valid_window(window: &UsageWindow) -> bool {
    window.used.is_finite()
        && (0.0..=1.0).contains(&window.used)
        // Empty when the provider reported no reset time.
        && window.resets_at.len() <= 128
        && !window.label.is_empty()
        && window.label.len() <= 128
}

/// The name the wire gives the provider, which is also its key in the table.
fn provider_name(provider: &UsageProvider) -> Result<String> {
    match serde_json::to_value(provider)? {
        serde_json::Value::String(name) => Ok(name),
        _ => Err(Error::InvalidUsageReading),
    }
}

fn provider_from_name(name: &str) -> Result<UsageProvider> {
    serde_json::from_value(serde_json::Value::String(name.to_owned()))
        .map_err(|_| Error::InvalidUsageReading)
}

/// The order the meters are listed in.
fn provider_rank(provider: &UsageProvider) -> u8 {
    match provider {
        UsageProvider::Claude => 0,
        UsageProvider::Codex => 1,
        UsageProvider::Cursor => 2,
        UsageProvider::Opencode => 3,
    }
}
