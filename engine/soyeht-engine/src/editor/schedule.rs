//! When the editor's background effects (autosave, disk watch) are due, and the
//! retry backoff for the ones that fail.
//!
//! An effect that keeps failing, for example because the file or its workspace
//! folder is gone, must not run and log on every tick. Each effect of each
//! open file owns a [`Backoff`]: every consecutive failure doubles the wait up
//! to `MAX_RETRY_MS` and only some failures are reported (see `crate::backoff`).

use super::service::{Service, autosave_ms};
use crate::backoff::Backoff;

/// Interval between two disk checks of a watched file.
pub(super) const WATCH_INTERVAL_MS: i64 = 250;

/// Longest wait between two attempts of a failing effect.
const MAX_RETRY_MS: i64 = 60_000;

/// The backoff of a file's autosave: its normal interval is the autosave delay.
pub(super) fn save_backoff() -> Backoff {
    Backoff::new(autosave_ms(), MAX_RETRY_MS)
}

/// The backoff of a file's disk watch.
pub(super) const fn watch_backoff() -> Backoff {
    Backoff::new(WATCH_INTERVAL_MS, MAX_RETRY_MS)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    Save,
    Watch,
}

impl Effect {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Save => "save",
            Self::Watch => "watch",
        }
    }
}

/// What the caller must log for one failed attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Failure {
    pub attempts: u32,
    pub retry_in_ms: i64,
    pub report: bool,
}

impl Service {
    pub fn due_saves(&self, now_ms: i64) -> Vec<String> {
        self.open
            .iter()
            .filter(|(_, item)| item.save_due_ms.is_some_and(|due| due <= now_ms))
            .map(|(file_id, _)| file_id.clone())
            .collect()
    }

    pub fn due_watches(&self, now_ms: i64) -> Vec<String> {
        self.open
            .iter()
            .filter(|(_, item)| item.file.watching && item.watch_due_ms <= now_ms)
            .map(|(file_id, _)| file_id.clone())
            .collect()
    }

    pub fn checked_watch(&mut self, file_id: &str, now_ms: i64) {
        if let Some(item) = self.open.get_mut(file_id) {
            item.watch_due_ms = now_ms.saturating_add(WATCH_INTERVAL_MS);
        }
    }

    pub fn defer_save(&mut self, file_id: &str, now_ms: i64) {
        if let Some(item) = self.open.get_mut(file_id) {
            item.save_due_ms = Some(now_ms.saturating_add(autosave_ms()));
        }
    }

    /// Records a failed attempt and moves the next attempt out by the backoff.
    /// `None` when the file is no longer open.
    pub(crate) fn effect_failed(
        &mut self,
        file_id: &str,
        effect: Effect,
        now_ms: i64,
    ) -> Option<Failure> {
        let item = self.open.get_mut(file_id)?;
        let backoff = match effect {
            Effect::Save => &mut item.save_backoff,
            Effect::Watch => &mut item.watch_backoff,
        };
        backoff.fail();
        let failure = Failure {
            attempts: backoff.attempts(),
            retry_in_ms: backoff.next_delay_ms(),
            report: backoff.should_log(),
        };
        let due = now_ms.saturating_add(failure.retry_in_ms);
        match effect {
            Effect::Save => item.save_due_ms = Some(due),
            Effect::Watch => item.watch_due_ms = due,
        }
        Some(failure)
    }

    /// Ends the failure streak of `effect`; returns the attempts it took.
    pub(crate) fn effect_succeeded(&mut self, file_id: &str, effect: Effect) -> Option<u32> {
        let item = self.open.get_mut(file_id)?;
        match effect {
            Effect::Save => item.save_backoff.succeed(),
            Effect::Watch => item.watch_backoff.succeed(),
        }
    }

    #[cfg(test)]
    pub(crate) fn effect_attempts(&self, file_id: &str, effect: Effect) -> u32 {
        self.open.get(file_id).map_or(0, |item| match effect {
            Effect::Save => item.save_backoff.attempts(),
            Effect::Watch => item.watch_backoff.attempts(),
        })
    }
}
