mod claude;
mod codex;
mod cursor;

use crate::{Result, Window};
use soyeht_protocol::{UsageProvider, UsageSource};

pub(crate) struct ParsedReading {
    pub primary: Window,
    pub secondary: Option<Window>,
}

pub(crate) trait ProviderReader: Send + Sync {
    fn provider(&self) -> UsageProvider;
    fn sources(&self) -> &'static [UsageSource];
    fn parse(&self, source: UsageSource, payload: &[u8]) -> Result<ParsedReading>;
}

pub(crate) fn readers() -> Vec<Box<dyn ProviderReader>> {
    vec![
        Box::new(claude::Reader),
        Box::new(codex::Reader),
        Box::new(cursor::Reader),
    ]
}

fn json(payload: &[u8], source: UsageSource) -> Result<serde_json::Value> {
    serde_json::from_slice(payload).map_err(|_| crate::Error::InvalidResponse { source })
}

fn percent_fraction(value: f64, source: UsageSource) -> Result<f64> {
    let fraction = value / 100.0;
    if fraction.is_finite() && (0.0..=1.0).contains(&fraction) {
        Ok(fraction)
    } else {
        Err(crate::Error::InvalidWindow { source })
    }
}
