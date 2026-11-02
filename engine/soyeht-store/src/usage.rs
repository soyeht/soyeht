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
                [provider_name(provider)],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        raw.map(|(observed_at_unix_sec, payload)| {
            decode_stored(provider, observed_at_unix_sec, &payload)
        })
        .transpose()
    }

    pub fn usage_readings(&self) -> Result<Vec<StoredUsageReading>> {
        let mut statement = self.connection.prepare(
            "SELECT provider,observed_at_unix_sec,payload FROM usage_readings ORDER BY CASE provider WHEN 'claude' THEN 0 WHEN 'codex' THEN 1 WHEN 'cursor' THEN 2 WHEN 'opencode' THEN 3 END",
        )?;
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
            let expected = provider_from_name(&provider).ok_or(Error::InvalidUsageReading)?;
            readings.push(decode_stored(&expected, observed_at_unix_sec, &payload)?);
        }
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
                provider_name(&reading.provider),
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

fn provider_name(provider: &UsageProvider) -> &'static str {
    match provider {
        UsageProvider::Claude => "claude",
        UsageProvider::Codex => "codex",
        UsageProvider::Cursor => "cursor",
        UsageProvider::Opencode => "opencode",
    }
}

fn provider_from_name(name: &str) -> Option<UsageProvider> {
    match name {
        "claude" => Some(UsageProvider::Claude),
        "codex" => Some(UsageProvider::Codex),
        "cursor" => Some(UsageProvider::Cursor),
        "opencode" => Some(UsageProvider::Opencode),
        _ => None,
    }
}
