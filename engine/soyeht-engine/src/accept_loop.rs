//! What an accept loop does with the error of `accept`.
//!
//! A listener keeps accepting through the errors that do not end it, and ends, with the error, when
//! one does, so that whatever keeps the listener recovers it: the tailnet follower binds the
//! address again (`network::TailnetListeners`) and the service manager starts a loopback or local
//! engine again.
//!
//! The errors of `accept` are of three kinds:
//!
//! - **About one connection** (the peer reset or aborted it while it waited, the call was
//!   interrupted; on macOS an accepted connection with an empty address): the next `accept` has
//!   an answer at once, so the loop waits only the platform's `ACCEPT_ERROR_FLOOR` (it cannot spin
//!   a core). A
//!   run of them that goes on for [`CONNECTION_GIVE_UP`] with no connection accepted ends the
//!   listener.
//! - **The process is out of descriptors or memory** (`EMFILE`, `ENFILE`, `ENOBUFS`, `ENOMEM`):
//!   the listener is fine and a connection that closes frees a descriptor. The loop pauses, for
//!   longer while the errors go on, up to a cap, and never ends.
//! - **Anything else** (a closed descriptor, a socket that is not a listener, a dead interface):
//!   the same pause, and the listener ends after [`LISTENER_GIVE_UP`] of them in a row.
//!
//! Each kind keeps its own [`Streak`], so a run of one does not lengthen the pause of another. The
//! failures are logged through `soyeht_platform::log` with the error's kind, never its message:
//! the first of a streak, each change of kind and then at growing intervals. An `accept` that
//! works ends the streaks, and the recovery is logged once.

use crate::streak::Streak;
use soyeht_platform::log::{self, Failure as _, Line};
use soyeht_platform::net::ACCEPT_ERROR_FLOOR;
use std::{io, time::Duration};
use tokio::time::Instant;

/// The pause after the first failure is twice this; it doubles while the failures go on.
const PAUSE_BASE_MS: i64 = 50;
const PAUSE_CAP_MS: i64 = 5_000;
/// Errors about connections, with none accepted for this long, end the listener.
const CONNECTION_GIVE_UP: Duration = soyeht_platform::net::ACCEPT_GIVE_UP;
/// Consecutive errors of the third kind that end the listener.
const LISTENER_GIVE_UP: u32 = 8;
const FAILED_EVENT: &str = "engine.accept_failed";
const GAVE_UP_EVENT: &str = "engine.accept_gave_up";
const RECOVERED_EVENT: &str = "engine.accept_recovered";

/// A listener's connections, one at a time.
pub(crate) trait Incoming {
    type Connection;

    /// Waits for the next connection.
    fn accept(&mut self) -> impl Future<Output = io::Result<Self::Connection>> + Send;
}

impl Incoming for tokio::net::TcpListener {
    type Connection = (tokio::net::TcpStream, std::net::SocketAddr);

    fn accept(&mut self) -> impl Future<Output = io::Result<Self::Connection>> + Send {
        tokio::net::TcpListener::accept(self)
    }
}

impl Incoming for soyeht_platform::local::asynchronous::Listener {
    type Connection = soyeht_platform::local::asynchronous::Stream;

    fn accept(&mut self) -> impl Future<Output = io::Result<Self::Connection>> + Send {
        soyeht_platform::local::asynchronous::Listener::accept(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Connection,
    Resources,
    Listener,
}

impl Kind {
    fn of(error: &io::Error) -> Self {
        if soyeht_platform::net::accept_error_concerns_one_connection(error) {
            Self::Connection
        } else if exhausts_resources(error) {
            Self::Resources
        } else {
            Self::Listener
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Connection => "connection",
            Self::Resources => "resources",
            Self::Listener => "listener",
        }
    }
}

/// Whether the process is out of descriptors, buffers or memory.
fn exhausts_resources(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::OutOfMemory || error.raw_os_error().is_some_and(resource_code)
}

#[cfg(unix)]
fn resource_code(code: i32) -> bool {
    [libc::EMFILE, libc::ENFILE, libc::ENOBUFS, libc::ENOMEM].contains(&code)
}

/// `WSAEMFILE` and `WSAENOBUFS`.
#[cfg(windows)]
fn resource_code(code: i32) -> bool {
    [10024, 10055].contains(&code)
}

/// What to do after a failed `accept`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    Retry(Duration),
    GiveUp,
}

/// The failures of one listener's `accept`, in a row.
pub(crate) struct Failures {
    /// The listener, as a log field: its address, or `local`.
    listener: String,
    connection: Streak,
    resources: Streak,
    other: Streak,
    /// When the current run of errors about connections began, until an `accept` works or an
    /// error of another kind shows the listener alive.
    connections_since: Option<Instant>,
}

impl Failures {
    pub(crate) fn new(listener: impl Into<String>) -> Self {
        let streak = || Streak::retrying(PAUSE_BASE_MS, PAUSE_CAP_MS);
        Self {
            listener: listener.into(),
            connection: streak(),
            resources: streak(),
            other: streak(),
            connections_since: None,
        }
    }

    fn streak(&mut self, kind: Kind) -> &mut Streak {
        match kind {
            Kind::Connection => &mut self.connection,
            Kind::Resources => &mut self.resources,
            Kind::Listener => &mut self.other,
        }
    }

    /// Counts a failed `accept`, logs it when its streak says to, and says what the loop does
    /// next.
    fn failed(&mut self, error: &io::Error) -> Verdict {
        let kind = Kind::of(error);
        let name = kind.name();
        if kind == Kind::Connection {
            self.connections_since.get_or_insert_with(Instant::now);
        } else {
            self.connections_since = None;
        }
        let since = self.connections_since;
        let streak = self.streak(kind);
        let said = streak.fail(&error.failure_kind());
        let attempts = streak.attempts();
        // An error about a connection waits only the floor; the others wait their streak's pause.
        let pause = if kind == Kind::Connection {
            ACCEPT_ERROR_FLOOR
        } else {
            Duration::from_millis(streak.next_delay_ms().max(0).unsigned_abs())
        };
        if said {
            log::failure_with(
                FAILED_EVENT,
                &[
                    ("listener", &self.listener),
                    ("class", name),
                    ("attempts", &attempts.to_string()),
                ],
                error,
            );
        }
        match kind {
            Kind::Connection
                if since.is_some_and(|since| since.elapsed() >= CONNECTION_GIVE_UP) =>
            {
                Verdict::GiveUp
            }
            Kind::Listener if attempts >= LISTENER_GIVE_UP => Verdict::GiveUp,
            Kind::Connection | Kind::Resources | Kind::Listener => Verdict::Retry(pause),
        }
    }

    /// Ends the streaks after an `accept` that worked.
    fn accepted(&mut self) {
        self.connections_since = None;
        let failures: u32 = [&mut self.connection, &mut self.resources, &mut self.other]
            .into_iter()
            .filter_map(|streak| streak.succeed())
            .sum();
        if failures > 0 {
            Line::new(RECOVERED_EVENT)
                .field("listener", &self.listener)
                .field("failures", failures)
                .emit();
        }
    }

    /// The next connection of `listener`, however many failed `accept`s come first. An error that
    /// ends the listener is returned.
    pub(crate) async fn next<L: Incoming>(
        &mut self,
        listener: &mut L,
    ) -> io::Result<L::Connection> {
        loop {
            match listener.accept().await {
                Ok(connection) => {
                    self.accepted();
                    return Ok(connection);
                }
                Err(error) => match self.failed(&error) {
                    Verdict::Retry(pause) => tokio::time::sleep(pause).await,
                    Verdict::GiveUp => {
                        log::failure_with(GAVE_UP_EVENT, &[("listener", &self.listener)], &error);
                        return Err(error);
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// A listener that answers with what a test scripted, then waits for ever.
    struct Scripted(VecDeque<io::Result<u32>>);

    impl Incoming for Scripted {
        type Connection = u32;

        fn accept(&mut self) -> impl Future<Output = io::Result<u32>> + Send {
            let next = self.0.pop_front();
            async move {
                match next {
                    Some(result) => result,
                    None => std::future::pending().await,
                }
            }
        }
    }

    fn error(kind: io::ErrorKind) -> io::Result<u32> {
        Err(io::Error::from(kind))
    }

    /// What the loop returns after the scripted answers and how long it waited for it.
    async fn accept_after(answers: Vec<io::Result<u32>>) -> (io::Result<u32>, Duration) {
        let mut listener = Scripted(answers.into());
        let mut failures = Failures::new("test");
        let started = Instant::now();
        let accepted = failures.next(&mut listener).await;
        (accepted, started.elapsed())
    }

    fn failed(failures: &mut Failures, kind: io::ErrorKind) -> Verdict {
        failures.failed(&io::Error::from(kind))
    }

    #[tokio::test(start_paused = true)]
    async fn a_connection_that_failed_in_the_queue_does_not_stop_the_loop() {
        let answers = vec![
            error(io::ErrorKind::ConnectionAborted),
            error(io::ErrorKind::ConnectionReset),
            error(io::ErrorKind::Interrupted),
            error(io::ErrorKind::InvalidInput),
            Ok(7),
        ];
        // Only the floor of 1 ms each: the errors were about the connections.
        let (accepted, waited) = accept_after(answers).await;
        assert_eq!(accepted.unwrap(), 7);
        assert_eq!(waited, Duration::from_millis(4));
    }

    #[tokio::test(start_paused = true)]
    async fn a_failure_of_the_process_is_waited_out_for_longer_each_time_and_then_accepted() {
        let answers = vec![
            error(io::ErrorKind::OutOfMemory),
            error(io::ErrorKind::OutOfMemory),
            error(io::ErrorKind::OutOfMemory),
            Ok(1),
        ];
        // 100 ms after the first failure, then 200 and 400.
        let (accepted, waited) = accept_after(answers).await;
        assert_eq!(accepted.unwrap(), 1);
        assert_eq!(waited, Duration::from_millis(700));
    }

    #[cfg(unix)]
    #[tokio::test(start_paused = true)]
    async fn running_out_of_descriptors_never_ends_the_listener() {
        let exhausted = || Err(io::Error::from_raw_os_error(libc::EMFILE));
        let mut answers: Vec<io::Result<u32>> = (0..100).map(|_| exhausted()).collect();
        answers.push(Ok(3));
        let (accepted, waited) = accept_after(answers).await;
        assert_eq!(accepted.unwrap(), 3);
        // The pause reached its cap and stayed there.
        assert!(waited > Duration::from_secs(400), "{waited:?}");
        assert!(exhausts_resources(&io::Error::from_raw_os_error(
            libc::ENFILE
        )));
        assert!(exhausts_resources(&io::Error::from_raw_os_error(
            libc::ENOBUFS
        )));
        assert!(exhausts_resources(&io::Error::from_raw_os_error(
            libc::ENOMEM
        )));
        assert!(!exhausts_resources(&io::Error::from_raw_os_error(
            libc::EBADF
        )));
    }

    #[cfg(unix)]
    #[tokio::test(start_paused = true)]
    async fn a_listener_that_is_dead_ends_with_its_error_after_a_bounded_run() {
        let dead = || Err(io::Error::from_raw_os_error(libc::EBADF));
        let answers: Vec<io::Result<u32>> = (0..20).map(|_| dead()).collect();
        let (accepted, waited) = accept_after(answers).await;
        let error = accepted.unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::EBADF));
        // Seven pauses (100, 200, 400, 800, 1600, 3200 and 5000 ms), then the eighth error ends it.
        assert_eq!(waited, Duration::from_millis(11_300));
    }

    #[tokio::test(start_paused = true)]
    async fn a_run_of_connection_errors_with_no_connection_ends_the_listener() {
        let answers: Vec<io::Result<u32>> = (0..70_000)
            .map(|_| error(io::ErrorKind::ConnectionReset))
            .collect();
        let (accepted, waited) = accept_after(answers).await;
        assert_eq!(accepted.unwrap_err().kind(), io::ErrorKind::ConnectionReset);
        assert!(waited >= CONNECTION_GIVE_UP, "{waited:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn the_time_of_connection_errors_does_not_count_the_time_of_other_failures() {
        let mut failures = Failures::new("test");
        let exhausted = io::Error::from(io::ErrorKind::OutOfMemory);
        assert!(matches!(failures.failed(&exhausted), Verdict::Retry(_)));
        tokio::time::advance(Duration::from_secs(70)).await;
        // The first error about a connection starts its own run.
        assert_eq!(
            failed(&mut failures, io::ErrorKind::ConnectionAborted),
            Verdict::Retry(ACCEPT_ERROR_FLOOR)
        );
        // A run that was interrupted by another kind of error starts again.
        tokio::time::advance(Duration::from_secs(50)).await;
        assert!(matches!(failures.failed(&exhausted), Verdict::Retry(_)));
        tokio::time::advance(Duration::from_secs(50)).await;
        assert_eq!(
            failed(&mut failures, io::ErrorKind::ConnectionAborted),
            Verdict::Retry(ACCEPT_ERROR_FLOOR)
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_errors_accept_documents_for_a_connection_are_not_the_listeners() {
        for code in [
            libc::EPROTO,
            libc::ENETDOWN,
            libc::EHOSTUNREACH,
            libc::EOPNOTSUPP,
        ] {
            assert_eq!(
                Kind::of(&io::Error::from_raw_os_error(code)),
                Kind::Connection,
                "{code}"
            );
        }
    }

    #[test]
    fn errors_about_connections_do_not_lengthen_the_pause_of_the_failures_after_them() {
        let mut failures = Failures::new("test");
        for _ in 0..40 {
            assert_eq!(
                failed(&mut failures, io::ErrorKind::ConnectionReset),
                Verdict::Retry(ACCEPT_ERROR_FLOOR)
            );
        }
        assert_eq!(
            failed(&mut failures, io::ErrorKind::Other),
            Verdict::Retry(Duration::from_millis(100))
        );
    }

    #[test]
    fn the_kinds_of_error_are_told_apart() {
        // The list itself is the platform's; here the kinds the loop treats differently.
        for kind in [io::ErrorKind::ConnectionReset, io::ErrorKind::InvalidInput] {
            assert_eq!(
                Kind::of(&io::Error::from(kind)),
                Kind::Connection,
                "{kind:?}"
            );
        }
        assert_eq!(
            Kind::of(&io::Error::from(io::ErrorKind::OutOfMemory)),
            Kind::Resources
        );
        assert_eq!(
            Kind::of(&io::Error::from(io::ErrorKind::Other)),
            Kind::Listener
        );
        assert_eq!(
            Kind::of(&io::Error::from(io::ErrorKind::PermissionDenied)),
            Kind::Listener
        );
    }

    #[test]
    fn the_log_says_the_first_failure_and_each_change_of_kind_not_each_failure() {
        let mut streak = Streak::retrying(PAUSE_BASE_MS, PAUSE_CAP_MS);
        let said: Vec<bool> = (0..9).map(|_| streak.fail("Other")).collect();
        // The first, then attempts 2, 4 and 8.
        assert_eq!(
            said,
            [true, true, false, true, false, false, false, true, false]
        );
        assert!(streak.fail("OutOfMemory"));
    }

    #[tokio::test]
    async fn a_real_listener_hands_over_its_connections() {
        let mut listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let client = tokio::spawn(tokio::net::TcpStream::connect(address));
        let mut failures = Failures::new(address.to_string());
        let (_, peer) = failures.next(&mut listener).await.unwrap();
        let connected = client.await.unwrap().unwrap();
        assert_eq!(peer, connected.local_addr().unwrap());
    }
}
