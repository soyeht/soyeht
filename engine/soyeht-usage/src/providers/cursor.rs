use super::{ParsedReading, ProviderReader};
use crate::{Error, Result, Window};
use soyeht_protocol::{UsageProvider, UsageSource};

const SOURCES: &[UsageSource] = &[UsageSource::CursorCurrentPeriod];

pub(crate) struct Reader;

impl ProviderReader for Reader {
    fn provider(&self) -> UsageProvider {
        UsageProvider::Cursor
    }

    fn sources(&self) -> &'static [UsageSource] {
        SOURCES
    }

    fn parse(&self, source: UsageSource, payload: &[u8]) -> Result<ParsedReading> {
        let document = super::json(payload, source.clone())?;
        let result = document.get("result").unwrap_or(&document);
        let percent = result["planUsage"]["totalPercentUsed"]
            .as_f64()
            .ok_or_else(|| Error::InvalidResponse {
                source: source.clone(),
            })?;
        let reset = result["billingCycleEnd"]
            .as_str()
            .ok_or_else(|| Error::InvalidResponse {
                source: source.clone(),
            })?;
        let primary = Window::checked(
            super::percent_fraction(percent, source.clone())?,
            reset,
            "Billing cycle",
            source,
        )?;
        Ok(ParsedReading {
            primary,
            secondary: None,
        })
    }
}
