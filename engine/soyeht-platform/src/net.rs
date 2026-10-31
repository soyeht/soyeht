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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

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
