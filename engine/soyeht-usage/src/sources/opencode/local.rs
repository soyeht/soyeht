//! What OpenCode recorded on this device, for a provider with no plan limit a reader exists for.
//!
//! `opencode stats` prints the same totals but only as text, so the database it reads is read
//! directly, read-only (`foreign_db`). The totals are of assistant messages of the last
//! `usageActivityDays` days and of the provider OpenCode is set to (every provider when none is
//! known). They are a count: they say nothing about a limit, so the reading has no window and no
//! percentage. A provider that bills nothing per token records a cost of zero, and no cost is
//! shown.

use crate::Activity;
use crate::host::{Host, LocalUsageRead};
use crate::sources::{Attempt, Failure, ParsedReading, ReadContext, Source};
use soyeht_protocol::{UsageSource, limits::USAGE_ACTIVITY_DAYS};

pub(super) struct LocalHistorySource;

impl Source for LocalHistorySource {
    fn id(&self) -> UsageSource {
        UsageSource::OpencodeLocalHistory
    }

    fn read(&self, host: &dyn Host, context: ReadContext) -> Attempt {
        if !super::installed(host) {
            return Err(Failure::ToolNotFound);
        }
        let database = super::database(host).ok_or(Failure::NoData)?;
        let provider = super::active_provider(host);
        let since_ms = i64::try_from(context.now_sec)
            .unwrap_or(i64::MAX)
            .saturating_sub(i64::try_from(USAGE_ACTIVITY_DAYS).unwrap_or(0) * 86_400)
            .saturating_mul(1000);
        match host.opencode_usage(&database, since_ms, provider.as_deref()) {
            LocalUsageRead::Totals(totals) => {
                let label = format!("Last {USAGE_ACTIVITY_DAYS} days on this device");
                let cost = (totals.cost_usd > 0.0).then_some(totals.cost_usd);
                let activity = Activity::checked(label, totals.tokens, cost)
                    .ok_or(Failure::InvalidResponse)?;
                Ok(ParsedReading {
                    primary: None,
                    secondary: None,
                    activity: Some(activity),
                })
            }
            // Installed, but no history this reader can read yet.
            LocalUsageRead::NoDatabase | LocalUsageRead::NoHistory => Err(Failure::NoData),
            LocalUsageRead::Unreadable => Err(Failure::Failed),
        }
    }
}
