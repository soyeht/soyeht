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
    time::Duration,
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
