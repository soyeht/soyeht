//! What a hold of a lock spent its time on.
//!
//! A slow hold of the engine's Core lock is only useful in the log when it says why. The guard of
//! the lock calls [`start`] when it takes the lock and [`finish`] when it lets go; in between, the
//! code that waits for something (a SQLite statement, a segment file, the editor's file port)
//! reports how long with [`timed`] or, for statements, [`statement_started`] and
//! [`statement_ended`]. The costs live in the thread that holds the lock, so a hold is never charged
//! for work another thread does, and work on a thread that holds no lock is not timed at all.

use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

/// What a charged stretch of time was spent on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cost {
    /// A SQLite statement other than a commit.
    Sql,
    /// A SQLite `COMMIT`: the write that waits for the disk.
    Commit,
    /// A read or an append of a terminal's segment file.
    Segment,
    /// A call of the editor's file port.
    File,
}

/// The time a hold spent on each [`Cost`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Costs {
    pub sql: Duration,
    pub commit: Duration,
    pub segment: Duration,
    pub file: Duration,
}

impl Costs {
    fn add(&mut self, cost: Cost, took: Duration) {
        let total = match cost {
            Cost::Sql => &mut self.sql,
            Cost::Commit => &mut self.commit,
            Cost::Segment => &mut self.segment,
            Cost::File => &mut self.file,
        };
        *total = total.saturating_add(took);
    }
}

/// The most statements that may be open at once in one hold. A statement that is started and never
/// finishes (a connection that is closed under it) cannot grow the stack past this.
const OPEN_STATEMENTS: usize = 64;

#[derive(Default)]
struct Account {
    costs: Costs,
    /// Statements started and not yet finished, newest last: they nest, as an iteration that runs
    /// another query inside its loop.
    statements: Vec<(Instant, Cost)>,
    /// Depth of [`timed`] calls in progress, so a call inside another is charged once.
    timing: u32,
}

thread_local! {
    static ACCOUNT: RefCell<Option<Account>> = const { RefCell::new(None) };
}

/// Starts charging this thread's time. Call it when the lock is taken.
pub fn start() {
    ACCOUNT.with(|account| *account.borrow_mut() = Some(Account::default()));
}

/// Stops charging and returns what was charged since [`start`].
pub fn finish() -> Costs {
    ACCOUNT.with(|account| {
        account
            .borrow_mut()
            .take()
            .map(|a| a.costs)
            .unwrap_or_default()
    })
}

fn charging() -> bool {
    ACCOUNT.with(|account| account.borrow().is_some())
}

fn with_account(change: impl FnOnce(&mut Account)) {
    ACCOUNT.with(|account| {
        if let Some(account) = account.borrow_mut().as_mut() {
            change(account);
        }
    });
}

/// Runs `work` and charges its time to `cost`, when this thread holds a lock that is being
/// measured. A call inside another `timed` call is not charged again.
pub fn timed<T>(cost: Cost, work: impl FnOnce() -> T) -> T {
    if !charging() {
        return work();
    }
    let mut outermost = false;
    with_account(|account| {
        outermost = account.timing == 0;
        account.timing += 1;
    });
    let started = Instant::now();
    let value = work();
    let took = started.elapsed();
    with_account(|account| {
        account.timing = account.timing.saturating_sub(1);
        if outermost {
            account.costs.add(cost, took);
        }
    });
    value
}

/// A SQLite statement began to run; `commit` says whether it is a `COMMIT`.
pub fn statement_started(commit: bool) {
    let cost = if commit { Cost::Commit } else { Cost::Sql };
    with_account(|account| {
        if account.statements.len() < OPEN_STATEMENTS {
            account.statements.push((Instant::now(), cost));
        }
    });
}

/// The statement that began last has finished.
pub fn statement_ended() {
    with_account(|account| {
        if let Some((started, cost)) = account.statements.pop() {
            account.costs.add(cost, started.elapsed());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    const PAUSE: Duration = Duration::from_millis(20);

    #[test]
    fn time_is_charged_to_its_cost_while_a_hold_is_measured() {
        start();
        timed(Cost::File, || thread::sleep(PAUSE));
        let costs = finish();
        assert!(costs.file >= PAUSE, "{costs:?}");
        assert_eq!(
            (costs.sql, costs.commit, costs.segment),
            (Duration::ZERO, Duration::ZERO, Duration::ZERO)
        );

        start();
        statement_started(false);
        thread::sleep(PAUSE);
        statement_ended();
        let costs = finish();
        assert!(costs.sql >= PAUSE, "{costs:?}");
        assert_eq!((costs.commit, costs.file), (Duration::ZERO, Duration::ZERO));

        start();
        statement_started(true);
        thread::sleep(PAUSE);
        statement_ended();
        timed(Cost::Segment, || thread::sleep(PAUSE));
        let costs = finish();
        assert!(costs.commit >= PAUSE, "{costs:?}");
        assert!(costs.segment >= PAUSE, "{costs:?}");
        assert_eq!(costs.sql, Duration::ZERO);
    }

    #[test]
    fn a_call_inside_another_is_charged_once_to_the_outer_cost() {
        start();
        timed(Cost::File, || {
            timed(Cost::Segment, || thread::sleep(PAUSE));
        });
        let costs = finish();
        assert!(costs.file >= PAUSE, "{costs:?}");
        assert_eq!(costs.segment, Duration::ZERO, "{costs:?}");
    }

    #[test]
    fn nothing_is_charged_when_no_hold_is_measured_and_a_hold_starts_clean() {
        timed(Cost::File, || thread::sleep(PAUSE));
        statement_started(false);
        statement_ended();
        start();
        assert_eq!(finish(), Costs::default());
        assert_eq!(finish(), Costs::default());
    }

    #[test]
    fn a_statement_that_never_ends_does_not_grow_the_stack() {
        start();
        for _ in 0..(OPEN_STATEMENTS * 3) {
            statement_started(false);
        }
        let open = ACCOUNT.with(|account| account.borrow().as_ref().map(|a| a.statements.len()));
        assert_eq!(open, Some(OPEN_STATEMENTS));
        finish();
    }
}
