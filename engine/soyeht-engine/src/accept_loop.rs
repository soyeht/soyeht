//! What an accept loop does with the error of `accept`.
//!
//! A connection that the peer reset while it waited in the queue, a process out of descriptors
//! and an interrupted call all fail one `accept`, not the listener. The loop of a listener keeps
//! accepting: it logs the failure through a [`Streak`] (the first one and each change of kind,
//! then at growing intervals, never the error's message), and after a failure that is not about
//! one connection it pauses before it asks again, for longer while the failures go on, up to a
//! cap. An accept that works ends the streak, and the recovery is logged once.

use crate::streak::Streak;
use soyeht_platform::log::{self, Failure as _, Line};
use std::{io, time::Duration};

/// The pause after the first failure is twice this; it doubles while the failures go on.
const PAUSE_BASE_MS: i64 = 50;
const PAUSE_CAP_MS: i64 = 5_000;
const FAILED_EVENT: &str = "engine.accept_failed";
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

/// The failures of one listener's `accept`, in a row.
pub(crate) struct Failures {
    listener: &'static str,
    streak: Streak,
}

impl Failures {
    pub(crate) const fn new(listener: &'static str) -> Self {
        Self {
            listener,
            streak: Streak::retrying(PAUSE_BASE_MS, PAUSE_CAP_MS),
        }
    }

    /// Counts a failed `accept`, logs it when the streak says to, and returns how long to wait
    /// before the next one.
    fn failed(&mut self, error: &io::Error) -> Duration {
        let kind = error.failure_kind();
        let attempts = self.streak.attempts().saturating_add(1);
        if self.streak.fail(&kind) {
            log::failure_with(
                FAILED_EVENT,
                &[
                    ("listener", self.listener),
                    ("attempts", &attempts.to_string()),
                ],
                error,
            );
        }
        if soyeht_platform::net::accept_error_concerns_one_connection(error) {
            Duration::ZERO
        } else {
            let milliseconds = self.streak.next_delay_ms().max(0).unsigned_abs();
            Duration::from_millis(milliseconds)
        }
    }

    /// Ends the streak after an `accept` that worked.
    fn accepted(&mut self) {
        if let Some(failures) = self.streak.succeed() {
            Line::new(RECOVERED_EVENT)
                .field("listener", self.listener)
                .field("failures", failures)
                .emit();
        }
    }

    /// The next connection of `listener`, however many failed `accept`s come first.
    pub(crate) async fn next<L: Incoming>(&mut self, listener: &mut L) -> L::Connection {
        loop {
            match listener.accept().await {
                Ok(connection) => {
                    self.accepted();
                    return connection;
                }
                Err(error) => {
                    let pause = self.failed(&error);
                    if pause.is_zero() {
                        tokio::task::yield_now().await;
                    } else {
                        tokio::time::sleep(pause).await;
                    }
                }
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

    /// What the loop hands over after the scripted answers, how long it waited for it, and how
    /// many failures it still counts; the listener serves on afterwards.
    async fn accept_after(answers: Vec<io::Result<u32>>) -> (u32, Duration, u32) {
        let mut listener = Scripted(answers.into());
        let mut failures = Failures::new("test");
        let started = tokio::time::Instant::now();
        let accepted = failures.next(&mut listener).await;
        (accepted, started.elapsed(), failures.streak.attempts())
    }

    #[tokio::test(start_paused = true)]
    async fn a_connection_that_failed_in_the_queue_does_not_stop_the_loop() {
        let answers = vec![
            error(io::ErrorKind::ConnectionAborted),
            error(io::ErrorKind::ConnectionReset),
            error(io::ErrorKind::Interrupted),
            Ok(7),
        ];
        // Nothing waited: the errors were about the connections, not the listener.
        assert_eq!(accept_after(answers).await, (7, Duration::ZERO, 0));
    }

    #[tokio::test(start_paused = true)]
    async fn a_failure_of_the_process_is_waited_out_for_longer_each_time_and_then_accepted() {
        let answers = vec![
            error(io::ErrorKind::Other),
            error(io::ErrorKind::Other),
            error(io::ErrorKind::Other),
            Ok(1),
        ];
        // 100 ms after the first failure, then 200 and 400.
        assert_eq!(
            accept_after(answers).await,
            (1, Duration::from_millis(700), 0)
        );
    }

    #[test]
    fn the_pause_stops_growing_at_the_cap() {
        let mut failures = Failures::new("test");
        let other = io::Error::from(io::ErrorKind::Other);
        let pauses: Vec<Duration> = (0..12).map(|_| failures.failed(&other)).collect();
        assert_eq!(pauses[0], Duration::from_millis(100));
        assert_eq!(pauses[1], Duration::from_millis(200));
        assert!(pauses.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(pauses[11], Duration::from_millis(5_000));
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
        let mut failures = Failures::new("test");
        let (_, peer) = failures.next(&mut listener).await;
        let connected = client.await.unwrap().unwrap();
        assert_eq!(peer, connected.local_addr().unwrap());
    }
}
