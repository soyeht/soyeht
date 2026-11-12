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
/// The due time of a poll that has begun and not rested yet: already past.
const NOT_AT_REST: i64 = i64::MIN;

/// What each resting poll saw: the change counter, when to ask again and when
/// it began to rest.
#[derive(Default)]
pub(crate) struct Resting(RefCell<BTreeMap<&'static str, (u64, i64, Instant)>>);

impl Store {
    /// The number of rows this connection has written.
    pub fn change_counter(&self) -> u64 {
        self.connection.total_changes()
    }

    /// How long until a poll has to ask again, for a tick that sleeps (ADR 0147, step 1b): zero
    /// when a poll is not resting (it has begun and not rested, it found work, a write woke it,
    /// or its rest is over), else the time to its due time. `None` when no poll has begun yet or
    /// every one waits for a write. The end of a rest with no due time is not a wake of its own:
    /// the tick's ceiling, which is shorter than [`CEILING`], is what sees a write by another
    /// process.
    pub fn next_poll_wait(&self, now_ms: i64) -> Option<Duration> {
        let seen = self.change_counter();
        self.resting
            .0
            .borrow()
            .values()
            .filter_map(|(at, until_ms, since)| {
                if *at != seen || now_ms >= *until_ms || since.elapsed() >= CEILING {
                    return Some(Duration::ZERO);
                }
                let left = u64::try_from(until_ms.saturating_sub(now_ms)).ok()?;
                (*until_ms != i64::MAX).then(|| Duration::from_millis(left))
            })
            .min()
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
        if resting {
            return None;
        }
        // From here until it rests the poll is not at rest, which `next_poll_wait` reports.
        self.resting
            .0
            .borrow_mut()
            .entry(poll)
            .or_insert((seen, NOT_AT_REST, Instant::now()));
        Some(seen)
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

    fn open() -> (Store, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("soyeht-resting-{}", Uuid::new_v4()));
        (Store::open(&root).unwrap(), root)
    }

    #[test]
    fn the_tick_waits_for_the_poll_that_is_not_at_rest_and_for_the_due_time_of_a_rest() {
        let (store, root) = open();
        assert_eq!(store.next_poll_wait(0), None, "no poll has begun");

        // A poll that began and found work has not rested: it is due now.
        let seen = store.begin_poll("found_work", 0).unwrap();
        assert_eq!(store.next_poll_wait(0), Some(Duration::ZERO));

        // A rest ends at its due time, and the earliest of the polls wins.
        store.rest("found_work", seen, 300);
        assert_eq!(store.next_poll_wait(100), Some(Duration::from_millis(200)));
        assert_eq!(store.next_poll_wait(300), Some(Duration::ZERO));
        let other = store.begin_poll("other", 100).unwrap();
        store.rest("other", other, 150);
        assert_eq!(store.next_poll_wait(100), Some(Duration::from_millis(50)));

        // A rest with no due time waits for a write, and the tick's ceiling covers the rest.
        store.rest("found_work", seen, i64::MAX);
        store.rest("other", other, i64::MAX);
        assert_eq!(store.next_poll_wait(100), None);

        // A write ends every rest.
        store
            .connection
            .execute("UPDATE metadata SET value=value WHERE key='db_id'", [])
            .unwrap();
        assert_eq!(store.next_poll_wait(100), Some(Duration::ZERO));
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

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
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}
