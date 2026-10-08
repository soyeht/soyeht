//! Polls that found nothing to do.
//!
//! The engine asks the database "is anything due?" on every tick. When the
//! answer is "no", asking again is wasted work until the database changes or a
//! known due time arrives. A poll records the change counter before it reads;
//! if the counter still matches later, nothing was written since, so the same
//! question has the same answer. The engine's own change counter sees every
//! write the engine makes; a write by another process (the command line tool,
//! a test probe) is seen at the latest when the rest reaches `CEILING`.

use crate::database::{Result, Store};
use std::{cell::RefCell, collections::BTreeMap, time::Duration, time::Instant};

/// The longest a poll rests, however far off its next due time is.
const CEILING: Duration = Duration::from_secs(5);

/// What each resting poll saw: the change counter, when to ask again and when
/// it began to rest.
#[derive(Default)]
pub(crate) struct Resting(RefCell<BTreeMap<&'static str, (u64, i64, Instant)>>);

impl Store {
    /// The number of rows this connection has written.
    fn change_counter(&self) -> u64 {
        self.connection.total_changes()
    }

    /// Starts a poll: `None` while it rests (it found nothing, nothing was
    /// written since and its next due time has not arrived), else the change
    /// counter to hand to `rest`. The counter is read before the poll queries,
    /// so a write by the poll itself keeps it from resting.
    pub fn begin_poll(&self, poll: &'static str, now_ms: i64) -> Option<u64> {
        let seen = self.change_counter();
        let resting = self
            .resting
            .0
            .borrow()
            .get(poll)
            .is_some_and(|(at, until_ms, since)| {
                *at == seen && now_ms < *until_ms && since.elapsed() < CEILING
            });
        (!resting).then_some(seen)
    }

    /// Records that `poll`, begun at `seen`, found nothing to do until
    /// `until_ms` (`i64::MAX` when only a write can give it work).
    pub fn rest(&self, poll: &'static str, seen: u64, until_ms: i64) {
        let entry = (seen, until_ms, Instant::now());
        self.resting.0.borrow_mut().insert(poll, entry);
    }

    /// Answers a question that depends only on the rows and the time. While
    /// the last answer was empty and nothing changed, it is still empty.
    pub(crate) fn rested_rows<T>(
        &self,
        poll: &'static str,
        now_ms: i64,
        query: impl FnOnce() -> Result<Vec<T>>,
        next_due: impl FnOnce(&Self) -> Result<Option<i64>>,
    ) -> Result<Vec<T>> {
        let Some(seen) = self.begin_poll(poll, now_ms) else {
            return Ok(Vec::new());
        };
        let rows = query()?;
        if rows.is_empty() {
            self.rest(poll, seen, next_due(self)?.unwrap_or(i64::MAX));
        }
        Ok(rows)
    }

    /// When the next task loop is due.
    pub(crate) fn next_loop_tick_ms(&self) -> Result<Option<i64>> {
        self.earliest("SELECT MIN(next_tick_ms) FROM task_loops")
    }

    /// When the next blocked task retry is due.
    pub(crate) fn next_task_retry_ms(&self) -> Result<Option<i64>> {
        self.earliest("SELECT MIN(due_ms) FROM task_block_retries")
    }

    /// When the next unanswered web permission request expires.
    pub(crate) fn next_web_permission_expiry_ms(&self) -> Result<Option<i64>> {
        self.earliest("SELECT MIN(expires_ms) FROM web_permission_requests WHERE result IS NULL")
    }

    fn earliest(&self, query: &str) -> Result<Option<i64>> {
        Ok(self
            .connection
            .prepare_cached(query)?
            .query_row([], |row| row.get(0))?)
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn a_poll_rests_until_a_write_or_its_due_time() {
        let root = std::env::temp_dir().join(format!("soyeht-resting-{}", Uuid::new_v4()));
        let store = Store::open(&root).unwrap();
        let seen = store.begin_poll("poll", 0).expect("a new poll runs");
        store.rest("poll", seen, 100);
        assert!(store.begin_poll("poll", 99).is_none());
        assert!(store.begin_poll("poll", 100).is_some());
        store.rest("poll", store.change_counter(), i64::MAX);
        assert!(store.begin_poll("poll", 1_000_000).is_none());
        store
            .connection
            .execute("UPDATE metadata SET value=value WHERE key='db_id'", [])
            .unwrap();
        assert!(
            store.begin_poll("poll", 0).is_some(),
            "a write wakes the poll"
        );
        assert_eq!(store.next_loop_tick_ms().unwrap(), None);
        fs::remove_dir_all(root).unwrap();
    }
}
