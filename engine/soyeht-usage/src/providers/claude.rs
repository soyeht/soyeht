use super::{ParsedReading, ProviderReader};
use crate::{Error, Result, Window};
use soyeht_protocol::{UsageProvider, UsageSource};

const SOURCES: &[UsageSource] = &[
    UsageSource::ClaudeCliUsage,
    UsageSource::ClaudeOauthEndpoint,
    UsageSource::ClaudeDesktopCache,
];

pub(crate) struct Reader;

impl ProviderReader for Reader {
    fn provider(&self) -> UsageProvider {
        UsageProvider::Claude
    }

    fn sources(&self) -> &'static [UsageSource] {
        SOURCES
    }

    fn parse(&self, source: UsageSource, payload: &[u8]) -> Result<ParsedReading> {
        if source != UsageSource::ClaudeCliUsage {
            return Err(Error::UnverifiedResponse { source });
        }
        parse_cli(payload, source)
    }
}

fn parse_cli(payload: &[u8], source: UsageSource) -> Result<ParsedReading> {
    let document = super::json(payload, source.clone())?;
    let text = document["result"]
        .as_str()
        .ok_or_else(|| Error::InvalidResponse {
            source: source.clone(),
        })?;
    let primary = parse_line(text, "Current session:", "Current session", source.clone())?;
    let secondary = parse_line(text, "Current week (all models):", "Current week", source)?;
    Ok(ParsedReading {
        primary,
        secondary: Some(secondary),
    })
}

fn parse_line(text: &str, prefix: &str, label: &str, source: UsageSource) -> Result<Window> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with(prefix))
        .ok_or_else(|| Error::InvalidResponse {
            source: source.clone(),
        })?;
    let remainder = line[prefix.len()..].trim();
    let (used_text, reset_text) =
        remainder
            .split_once(" used · resets ")
            .ok_or_else(|| Error::InvalidResponse {
                source: source.clone(),
            })?;
    let percent = used_text
        .trim_end_matches('%')
        .parse::<f64>()
        .map_err(|_| Error::InvalidResponse {
            source: source.clone(),
        })?;
    Window::checked(percent / 100.0, reset_text, label, source)
}
