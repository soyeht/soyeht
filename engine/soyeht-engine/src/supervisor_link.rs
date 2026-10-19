//! The engine's link to the process supervisor: when a lost link is retried, and what
//! becomes of a pane the supervisor refuses to start again.
//!
//! Reconnecting runs under the Core lock (ADR 0079), so a supervisor that cannot be reached
//! or used is retried with a growing delay instead of on every tick, and one pane that cannot
//! be restarted never keeps the whole link from being established.

use crate::{
    core::Core,
    errors::{Error, Result},
    streak::Streak,
};
use soyeht_platform::log::{self, Failure as _};
use soyeht_protocol::PaneState;
use soyeht_ptyd::wire_kind;

/// Delay after the first failed attempt is twice this; the delay doubles up to the cap.
const RETRY_BASE_MS: i64 = 100;
const RETRY_CAP_MS: i64 = 2_000;
/// How long a link must stay up before the next loss is treated as a new problem. A link
/// that is lost sooner counts as another failed attempt, so a supervisor that accepts the
/// hello and then drops is not reconnected without delay.
const STABLE_MS: i64 = 10_000;

/// Why a pane that could not be restarted says it stopped; it names no path or command.
const NOT_RESTARTED: &str = "The pane's working folder or command is missing";

/// What the tick does about the supervisor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Link {
    Up,
    /// There is no link; the caller skips the work that needs one. The failure was logged
    /// here, once per change of kind and at growing intervals, never by the tick.
    Down,
}

pub(crate) struct Retry {
    streak: Streak,
    due_ms: i64,
    /// When the current link was established, until it has been up for [`STABLE_MS`].
    connected_ms: Option<i64>,
}

impl Default for Retry {
    fn default() -> Self {
        Self {
            streak: Streak::retrying(RETRY_BASE_MS, RETRY_CAP_MS),
            due_ms: 0,
            connected_ms: None,
        }
    }
}

impl Core {
    /// Connects to the supervisor when there is no link and the retry delay has passed. A
    /// tick that falls inside the delay does not touch the supervisor. A failed attempt is
    /// logged by the retry streak and reported as [`Link::Down`], not as an error, so the
    /// tick's own failure log cannot alternate with it.
    pub(crate) fn ensure_supervisor(&mut self) -> Link {
        let now = self.now_ms();
        if self.ptyd.is_some() {
            self.settle_link(now);
            return Link::Up;
        }
        if let Some(since) = self.supervisor_retry.connected_ms.take() {
            // The link was up and is gone.
            if now - since >= STABLE_MS {
                self.supervisor_retry.streak.succeed();
            } else {
                self.supervisor_retry.streak.fail("link_lost");
                self.supervisor_retry.due_ms =
                    now.saturating_add(self.supervisor_retry.streak.next_delay_ms());
            }
        }
        if now < self.supervisor_retry.due_ms {
            return Link::Down;
        }
        match self.connect_supervisor() {
            Ok(()) => {
                self.supervisor_retry.connected_ms = Some(now);
                Link::Up
            }
            Err(error) => {
                if self.supervisor_retry.streak.fail(&error.failure_kind()) {
                    log::failure("engine.supervisor_unavailable", &error);
                }
                self.supervisor_retry.due_ms =
                    now.saturating_add(self.supervisor_retry.streak.next_delay_ms());
                Link::Down
            }
        }
    }

    /// Ends the failure streak once the link has stayed up for [`STABLE_MS`].
    fn settle_link(&mut self, now: i64) {
        if let Some(since) = self.supervisor_retry.connected_ms
            && now - since >= STABLE_MS
        {
            self.supervisor_retry.connected_ms = None;
            self.supervisor_retry.streak.succeed();
        }
    }

    /// Whether `error` is the supervisor answering a spawn with a refusal that repeating cannot
    /// fix, on a link that is still up: the folder or the command is missing or not allowed.
    /// Every other failure, including a spawn that failed for lack of a resource or time, and
    /// a lost link, may pass and is not proof the pane cannot be restored.
    pub(crate) fn refused_by_supervisor(&self, error: &Error) -> bool {
        self.ptyd.is_some()
            && matches!(
                error,
                Error::Supervisor(soyeht_ptyd::Error::Remote { kind, .. })
                    if kind == wire_kind::SPAWN_REFUSED
            )
    }

    /// Handles the failure of one pane's recovery. When the supervisor refused to spawn it
    /// (see [`Core::refused_by_supervisor`]), the pane is stopped and the other panes are
    /// recovered; any other failure still fails the whole reconciliation, which is retried.
    pub(crate) fn stop_unrestartable_pane(&mut self, id: &str, error: Error) -> Result<()> {
        if !self.refused_by_supervisor(&error) {
            return Err(error);
        }
        log::failure("engine.pane_not_restarted", &error);
        if self.agents.contains_key(id) {
            return self.fail_agent(id, NOT_RESTARTED);
        }
        let mut stopped = self.panes[id].stored.clone();
        stopped.pane.pid = None;
        stopped.instance_id = None;
        stopped.supervisor_id = None;
        stopped.pane.state = PaneState::Dead;
        stopped.pane.phrase = NOT_RESTARTED.into();
        self.save_runtime(id, stopped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{errors, ports::Clock, ports::testing::FakeSupervisor};
    use soyeht_platform::local::{Listener, connect};
    use soyeht_protocol::{frame::write_json, profile::DEV_PTYD_SOCKET};
    use soyeht_ptyd::wire::{Response, VERSION};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering},
    };

    /// A supervisor socket that accepts every connection and hangs up, after a welcome when
    /// `welcome` is set (so the connection works and the reconciliation then fails).
    struct HangUp {
        accepted: Arc<AtomicUsize>,
        stop: Arc<AtomicBool>,
        socket: std::path::PathBuf,
    }

    impl HangUp {
        fn start(directory: &std::path::Path, welcome: bool) -> Self {
            let socket = directory.join(DEV_PTYD_SOCKET);
            let listener = Listener::bind(&socket).unwrap();
            let accepted = Arc::new(AtomicUsize::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let (count, done) = (Arc::clone(&accepted), Arc::clone(&stop));
            std::thread::spawn(move || {
                while let Ok(mut stream) = listener.accept() {
                    if done.load(Ordering::SeqCst) {
                        break;
                    }
                    count.fetch_add(1, Ordering::SeqCst);
                    if welcome {
                        let hello = soyeht_protocol::frame::Frame::read_from(&mut stream);
                        assert!(hello.is_ok());
                        let welcome = Response::Welcome {
                            version: VERSION,
                            supervisor_id: "test".into(),
                            supervisor_generation: 1,
                        };
                        assert!(write_json(&mut stream, &welcome).is_ok());
                    }
                    drop(stream);
                }
            });
            Self {
                accepted,
                stop,
                socket,
            }
        }

        fn accepted(&self) -> usize {
            self.accepted.load(Ordering::SeqCst)
        }
    }

    impl Drop for HangUp {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            // Wakes the accepting thread so it sees the flag and ends.
            drop(connect(&self.socket));
        }
    }

    /// A clock the test moves.
    #[derive(Debug)]
    struct Manual(AtomicI64);

    impl Clock for Manual {
        fn now_ms(&self) -> i64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    fn short_directory() -> std::path::PathBuf {
        // A socket path has a short limit: keep the folder name short.
        let directory = std::env::temp_dir().join(format!(
            "sn-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    /// 500 ticks against a supervisor that fails every attempt reach it at most once, and the
    /// tick itself never reports the failure (the retry streak owns the log line).
    fn assert_one_attempt_per_burst(welcome: bool) {
        let directory = short_directory();
        let supervisor = HangUp::start(&directory, welcome);
        let mut core = Core::open(&directory).unwrap();
        let opened = supervisor.accepted();
        for _ in 0..500 {
            assert!(core.poll_output().is_ok());
        }
        // The retry delay starts at 200 ms: a burst of ticks reaches the supervisor once.
        let attempts = supervisor.accepted() - opened;
        assert!(attempts <= 1, "{attempts} connection attempts in one burst");
        assert!(core.ptyd.is_none());
    }

    #[test]
    fn an_unreachable_supervisor_is_not_retried_on_every_tick() {
        assert_one_attempt_per_burst(false);
    }

    #[test]
    fn a_supervisor_that_welcomes_and_drops_is_not_retried_on_every_tick() {
        assert_one_attempt_per_burst(true);
    }

    #[test]
    fn a_link_lost_soon_after_it_was_made_keeps_the_delay_and_a_stable_one_resets_it() {
        let directory = short_directory();
        let mut core = Core::open(&directory).unwrap();
        let clock = Arc::new(Manual(AtomicI64::new(1_000_000)));
        core.clock = clock.clone();
        core.supervisor_retry.connected_ms = Some(999_000);
        assert_eq!(core.ensure_supervisor(), Link::Down);
        assert_eq!(core.supervisor_retry.streak.attempts(), 1);
        assert!(core.supervisor_retry.due_ms > 1_000_000);

        // Up for longer than the stable time: the next loss starts over.
        core.supervisor_retry.due_ms = 0;
        core.supervisor_retry.connected_ms = Some(1_000_000 - STABLE_MS);
        assert_eq!(core.ensure_supervisor(), Link::Down);
        assert_eq!(core.supervisor_retry.streak.attempts(), 1);
        assert!(core.supervisor_retry.due_ms > 1_000_000);
        assert!(core.supervisor_retry.connected_ms.is_none());
    }

    fn remote(kind: &str) -> Error {
        Error::Supervisor(soyeht_ptyd::Error::Remote {
            kind: kind.into(),
            message: "x".into(),
        })
    }

    #[test]
    fn only_a_refused_spawn_stops_a_pane() {
        let directory = short_directory();
        let mut core = Core::open(&directory).unwrap();
        core.ptyd = Some(Box::new(FakeSupervisor::default()));
        assert!(core.refused_by_supervisor(&remote(wire_kind::SPAWN_REFUSED)));
        core.ptyd = None;
        assert!(!core.refused_by_supervisor(&remote(wire_kind::SPAWN_REFUSED)));
        core.ptyd = Some(Box::new(FakeSupervisor::default()));
        for kind in [
            // A spawn that failed for lack of a resource or time may succeed on the next try.
            wire_kind::SPAWN_FAILED,
            wire_kind::OPERATION_FAILED,
            wire_kind::NOT_FOUND,
            wire_kind::STALE_ENGINE_GENERATION,
        ] {
            assert!(!core.refused_by_supervisor(&remote(kind)), "{kind}");
            // The pane is left as it is and the reconciliation fails, to be retried.
            assert!(core.stop_unrestartable_pane("pane", remote(kind)).is_err());
        }
        assert!(!core.refused_by_supervisor(&errors::internal("x")));
    }
}
