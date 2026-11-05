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

/// Whether an error of `accept` concerns the one connection it took from the queue (the peer reset
/// or aborted it while it waited, the call was interrupted or timed out), not the listener or the
/// process. The next `accept` has an answer at once; any other error needs a pause before it.
pub fn accept_error_concerns_one_connection(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::Interrupted
            | io::ErrorKind::TimedOut
    )
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
        ] {
            assert!(accept_error_concerns_one_connection(&io::Error::from(kind)));
        }
        for kind in [
            io::ErrorKind::Other,
            io::ErrorKind::OutOfMemory,
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::InvalidInput,
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
