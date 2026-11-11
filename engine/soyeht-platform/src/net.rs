//! TCP listeners that no other program can take the port from.
//!
//! On Windows a program of any user can bind an address that another program already listens on
//! if it asks for `SO_REUSEADDR`, unless the first program asked for exclusive use, and the new
//! socket is then handed the connections meant for the first. The standard library asks for
//! neither, so every listener of the engine is made here. Elsewhere a second bind of a used
//! address fails, and the standard listener is the one this returns.

use std::{
    io,
    net::{SocketAddr, TcpListener},
    time::{Duration, Instant},
};

#[cfg(windows)]
mod exclusive;

/// Listens on `address` (port 0 picks a free one). On Windows no other program can bind the same
/// address and port while this listener lives.
pub fn listen(address: SocketAddr) -> io::Result<TcpListener> {
    #[cfg(windows)]
    {
        exclusive::exclusive_listener(address)
    }
    #[cfg(not(windows))]
    {
        TcpListener::bind(address)
    }
}

/// How long an accept loop waits after an error about one connection, so that a run of them
/// cannot spin a core.
pub const ACCEPT_ERROR_FLOOR: Duration = Duration::from_millis(1);

/// How long a run of errors about connections may go on with no connection accepted before the
/// listener is given up (the engine's own accept loops give up at the same time).
pub const ACCEPT_GIVE_UP: Duration = Duration::from_secs(60);

/// How far apart two errors can be and still be one run. A listener that has nothing to accept
/// waits inside `accept`, so errors that come far apart are errors of different connections, not
/// a listener that cannot accept.
pub const ACCEPT_RUN_GAP: Duration = Duration::from_secs(5);

/// Why a listener was given up: its accepts failed for [`ACCEPT_GIVE_UP`] with no connection
/// between. The log names it `accept_gave_up`.
#[derive(Debug)]
pub struct AcceptGaveUp;

impl std::fmt::Display for AcceptGaveUp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the listener accepted no connection while every accept failed")
    }
}

impl std::error::Error for AcceptGaveUp {}

/// A run of errors about one connection: the errors of `accept` that come one after another, none
/// more than [`ACCEPT_RUN_GAP`] after the one before, with no connection accepted between. Each is
/// waited out for [`ACCEPT_ERROR_FLOOR`]; a run that has gone on for the limit ends the listener
/// with [`AcceptGaveUp`], so that whatever keeps the listener recovers it. A listener that waited
/// them out for ever would spin at one pass a millisecond, unseen, while no client could connect;
/// one that counted from the first error it ever saw would give up a healthy listener that met
/// two unrelated errors a minute apart.
#[derive(Debug)]
pub struct ConnectionErrors {
    /// When the current run began, and when its last error came.
    run: Option<(Instant, Instant)>,
    give_up: Duration,
}

impl Default for ConnectionErrors {
    fn default() -> Self {
        Self::within(ACCEPT_GIVE_UP)
    }
}

impl ConnectionErrors {
    pub(crate) fn within(give_up: Duration) -> Self {
        Self { run: None, give_up }
    }

    /// An error about one connection, at the time of the call: the pause before the next
    /// `accept`, or the error that ends the listener.
    pub fn failed(&mut self) -> io::Result<Duration> {
        self.failed_at(Instant::now())
    }

    /// [`failed`](Self::failed) at `now`.
    pub fn failed_at(&mut self, now: Instant) -> io::Result<Duration> {
        let (began, _) = match self.run {
            Some((began, last)) if now.saturating_duration_since(last) <= ACCEPT_RUN_GAP => {
                (began, last)
            }
            _ => (now, now),
        };
        self.run = Some((began, now));
        if now.saturating_duration_since(began) >= self.give_up {
            return Err(io::Error::other(AcceptGaveUp));
        }
        Ok(ACCEPT_ERROR_FLOOR)
    }

    /// A connection was accepted, or the listener showed itself alive another way: the run is
    /// over.
    pub fn accepted(&mut self) {
        self.run = None;
    }
}

/// Whether `error` is the one that ends a listener that could accept nothing ([`AcceptGaveUp`]).
pub fn is_accept_give_up(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<AcceptGaveUp>())
}

/// Whether an error of `accept` concerns the one connection it took from the queue, not the
/// listener or the process. The next `accept` has an answer at once, after [`ACCEPT_ERROR_FLOOR`];
/// any other error needs a pause before it. They are the errors `accept(2)` documents for a
/// connection (the peer reset or aborted it while it waited, a protocol error on it, a network
/// error that, as the man page says, is to be treated like `EAGAIN`), the interrupted and timed
/// out calls, and, on macOS, a connection the peer reset before it was accepted, which comes back
/// with no address and which the runtime reports as invalid input.
pub fn accept_error_concerns_one_connection(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::Interrupted
            | io::ErrorKind::TimedOut
            | io::ErrorKind::InvalidInput
            | io::ErrorKind::NetworkDown
            | io::ErrorKind::HostUnreachable
            | io::ErrorKind::NetworkUnreachable
    ) || error.raw_os_error().is_some_and(connection_code)
}

/// The codes of `accept(2)` for a connection that have no kind of their own.
#[cfg(unix)]
fn connection_code(code: i32) -> bool {
    [
        libc::EPROTO,
        libc::ENETDOWN,
        libc::EHOSTUNREACH,
        libc::ENETUNREACH,
        libc::EHOSTDOWN,
        libc::ENOPROTOOPT,
        libc::EOPNOTSUPP,
    ]
    .contains(&code)
}

#[cfg(not(unix))]
fn connection_code(_: i32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn only_the_errors_about_one_connection_need_no_pause() {
        for kind in [
            io::ErrorKind::ConnectionAborted,
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::Interrupted,
            io::ErrorKind::TimedOut,
            io::ErrorKind::InvalidInput,
            io::ErrorKind::NetworkDown,
            io::ErrorKind::HostUnreachable,
            io::ErrorKind::NetworkUnreachable,
        ] {
            assert!(accept_error_concerns_one_connection(&io::Error::from(kind)));
        }
        #[cfg(unix)]
        for code in [
            libc::EPROTO,
            libc::ENETDOWN,
            libc::EHOSTUNREACH,
            libc::ENETUNREACH,
            libc::EHOSTDOWN,
            libc::ENOPROTOOPT,
            libc::EOPNOTSUPP,
        ] {
            assert!(accept_error_concerns_one_connection(
                &io::Error::from_raw_os_error(code)
            ));
        }
        #[cfg(unix)]
        for code in [libc::EBADF, libc::ENOTSOCK, libc::EMFILE, libc::EFAULT] {
            assert!(!accept_error_concerns_one_connection(
                &io::Error::from_raw_os_error(code)
            ));
        }
        for kind in [
            io::ErrorKind::Other,
            io::ErrorKind::OutOfMemory,
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::NotFound,
        ] {
            assert!(!accept_error_concerns_one_connection(&io::Error::from(
                kind
            )));
        }
    }

    #[test]
    fn a_run_of_connection_errors_ends_the_listener_and_an_accepted_connection_ends_the_run() {
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let mut errors = ConnectionErrors::within(Duration::from_secs(60));
        // An error every few seconds is a run; the sixtieth second ends the listener.
        for second in (0..60).step_by(4) {
            assert_eq!(errors.failed_at(at(second)).unwrap(), ACCEPT_ERROR_FLOOR);
        }
        let ended = errors.failed_at(at(60)).unwrap_err();
        assert!(is_accept_give_up(&ended));
        // The listener works again: a new run starts from zero.
        errors.accepted();
        assert_eq!(errors.failed_at(at(61)).unwrap(), ACCEPT_ERROR_FLOOR);
    }

    #[test]
    fn two_errors_far_apart_are_two_runs_and_end_nothing() {
        let start = Instant::now();
        let mut errors = ConnectionErrors::within(Duration::from_secs(60));
        errors.failed_at(start).unwrap();
        // A listener that waited a minute for a connection and then met one error.
        assert!(errors.failed_at(start + Duration::from_secs(61)).is_ok());
        assert!(errors.failed_at(start + Duration::from_secs(130)).is_ok());
        // A dead listener errors without pause: that run ends it.
        let mut dead = ConnectionErrors::within(Duration::from_secs(60));
        let ended = (0..=60)
            .map(|second| dead.failed_at(start + Duration::from_secs(second)))
            .find(Result::is_err);
        assert!(ended.is_some());
    }

    #[test]
    fn a_listener_takes_connections_on_loopback_and_refuses_a_second_listener_on_its_address() {
        let listener = listen(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = std::net::TcpStream::connect(address).unwrap();
        client.write_all(b"ping").unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let mut received = [0; 4];
        server.read_exact(&mut received).unwrap();
        assert_eq!(&received, b"ping");
        assert!(listen(address).is_err());
    }
}
