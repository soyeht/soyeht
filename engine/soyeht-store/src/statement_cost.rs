//! Charges the time of every statement to the hold of the Core lock that runs it, so a slow hold
//! says how much of it went to SQLite (`soyeht_platform::hold_cost`). SQLite's own timer has a
//! resolution of a millisecond, which misses a commit that takes less, so statements are timed
//! from the start and finish events with a monotonic clock.

use rusqlite::{
    Connection,
    trace::{TraceEvent, TraceEventCodes},
};

pub(crate) fn watch(connection: &Connection) {
    connection.trace_v2(
        TraceEventCodes::SQLITE_TRACE_STMT | TraceEventCodes::SQLITE_TRACE_PROFILE,
        Some(trace),
    );
    #[cfg(feature = "test-support")]
    if connection
        .commit_hook(Some(|| {
            crate::test_support::commit_made();
            false
        }))
        .is_err()
    {
        soyeht_platform::log::event("store.commit_hook_failed", &[]);
    }
}

fn trace(event: TraceEvent<'_>) {
    match event {
        TraceEvent::Stmt(_, sql) => {
            #[cfg(feature = "test-support")]
            crate::test_support::statement_started();
            #[cfg(test)]
            if is_commit(sql) {
                tests::COMMITS.with(|count| count.set(count.get() + 1));
            }
            soyeht_platform::hold_cost::statement_started(is_commit(sql));
        }
        TraceEvent::Profile(..) => soyeht_platform::hold_cost::statement_ended(),
        // Rows and closes are not statements; the enum may grow.
        _ => {}
    }
}

/// Whether `sql` is the statement that commits a transaction (`tx.commit()` runs `COMMIT`).
fn is_commit(sql: &str) -> bool {
    sql.trim_start()
        .get(..6)
        .is_some_and(|start| start.eq_ignore_ascii_case("COMMIT"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::is_commit;
    use std::cell::Cell;

    thread_local! {
        /// The `COMMIT`s started on this thread.
        pub(crate) static COMMITS: Cell<u64> = const { Cell::new(0) };
    }

    #[test]
    fn only_a_commit_is_a_commit() {
        assert!(is_commit("COMMIT"));
        assert!(is_commit("  commit;"));
        assert!(!is_commit("SELECT 1"));
        assert!(!is_commit("INSERT INTO t VALUES ('COMMIT')"));
        assert!(!is_commit("é"));
    }
}
