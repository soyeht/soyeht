//! The loopback flavour of the local endpoint, used on Windows. The code does not depend on the
//! system, so its tests run everywhere.

#![cfg_attr(not(windows), allow(dead_code))]

use super::{
    BindError,
    endpoint::{self, Admission, MAX_PENDING, Token},
};
use std::{
    io,
    net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    thread,
    time::Duration,
};

pub type Stream = TcpStream;

/// A loopback listener that hands out only the connections that proved they know the secret.
pub struct Listener {
    ready: Receiver<io::Result<Stream>>,
    address: SocketAddr,
    stop: Arc<AtomicBool>,
}

/// A loopback socket on a free port that no other process can bind, whatever it asks for.
pub(super) fn bind_loopback() -> io::Result<TcpListener> {
    #[cfg(windows)]
    {
        super::exclusive::bind_loopback()
    }
    #[cfg(not(windows))]
    {
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
    }
}

impl Listener {
    /// Listens on a free loopback port and writes the owner-only endpoint file at `path`.
    pub fn bind(path: &Path) -> Result<Self, BindError> {
        Self::bind_with(path, MAX_PENDING)
    }

    /// As [`Listener::bind`], with room for `capacity` connections mid-exchange.
    pub(super) fn bind_with(path: &Path, capacity: usize) -> Result<Self, BindError> {
        let tcp = bind_loopback()?;
        let address = tcp.local_addr()?;
        let token = endpoint::random()?;
        endpoint::publish(path, address.port(), &token)?;
        let (sender, ready) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        thread::Builder::new()
            .name("local-accept".into())
            .spawn(move || accept_loop(&tcp, token, &sender, &stopping, capacity))?;
        Ok(Self {
            ready,
            address,
            stop,
        })
    }

    /// Waits for the next connection that proved it knows the secret. Fails with the error
    /// that stopped the listener, and with `BrokenPipe` on every call after that.
    pub fn accept(&self) -> io::Result<Stream> {
        self.ready
            .recv()
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "listener stopped"))?
    }

    pub fn incoming(&self) -> impl Iterator<Item = io::Result<Stream>> + '_ {
        std::iter::repeat_with(|| self.accept())
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Wake the thread blocked in accept so it sees the flag and ends. When the connection
        // fails nothing is accepting any more, so the thread has already ended.
        drop(TcpStream::connect_timeout(
            &self.address,
            Duration::from_millis(200),
        ));
    }
}

fn accept_loop(
    tcp: &TcpListener,
    token: Token,
    ready: &Sender<io::Result<Stream>>,
    stop: &AtomicBool,
    capacity: usize,
) {
    let pending = Arc::new(Mutex::new(Admission::new(capacity)));
    for raw in tcp.incoming() {
        if stop.load(Ordering::Acquire) {
            return;
        }
        let raw = match raw {
            Ok(raw) => raw,
            Err(error) if endpoint::is_transient(&error) => continue,
            Err(error) => {
                // The listener is dead: hand the error to whoever accepts, then stop. If the
                // listener was dropped as well, nobody is left to hear it.
                drop(ready.send(Err(error)));
                return;
            }
        };
        let raw = Arc::new(raw);
        let Ok(mut waiting) = pending.lock() else {
            return;
        };
        let (number, evicted) = waiting.admit(Arc::clone(&raw));
        drop(waiting);
        if let Some(oldest) = evicted {
            // The peer may have closed it already; either way it ends here.
            drop(oldest.shutdown(Shutdown::Both));
        }
        let (ready, pending) = (ready.clone(), Arc::clone(&pending));
        let spawned = thread::Builder::new()
            .name("local-handshake".into())
            .spawn(move || {
                let proved = endpoint::serve(&raw, &token);
                if let Ok(mut waiting) = pending.lock() {
                    waiting.release(number);
                }
                if proved.is_ok()
                    && let Ok(stream) = raw.try_clone()
                {
                    // The listener was dropped when this fails: nobody is left to take it.
                    drop(ready.send(Ok(stream)));
                }
            });
        if spawned.is_err() {
            // Out of threads: this connection is dropped, which its client sees as a reset.
            thread::sleep(Duration::from_millis(50));
        }
    }
}

pub fn connect(path: &Path) -> io::Result<Stream> {
    endpoint::connect(path)
}

/// Two connected streams, for tests and in-process wiring.
pub fn pair() -> io::Result<(Stream, Stream)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let client = TcpStream::connect(listener.local_addr()?)?;
    let (server, _) = listener.accept()?;
    Ok((client, server))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::{
        endpoint::{NONCE_BYTES, TOKEN_BYTES},
        test_directory as directory,
    };
    use std::{
        fs,
        io::{Read, Write},
        time::Instant,
    };

    /// A client connects, says "ping", and the listener's side of it hears exactly that.
    fn ping_through(path: &Path, listener: &Listener) {
        let mut client = connect(path).unwrap();
        client.write_all(b"ping").unwrap();
        let mut server = listener.accept().unwrap();
        let mut received = [0; 4];
        server.read_exact(&mut received).unwrap();
        assert_eq!(&received, b"ping");
    }

    #[test]
    fn a_client_with_the_secret_is_accepted_and_one_without_is_not() {
        let path = directory().join("sync.sock");
        let listener = Listener::bind(&path).unwrap();
        let (address, _) = endpoint::read(&path).unwrap();
        let mut intruder = TcpStream::connect(address).unwrap();
        intruder.write_all(&[0; NONCE_BYTES]).unwrap();
        let mut reply = [0; 64];
        intruder.read_exact(&mut reply).unwrap();
        intruder.write_all(&[0; 32]).unwrap();
        ping_through(&path, &listener);
    }

    #[test]
    fn a_flood_of_silent_connections_does_not_keep_a_good_client_out() {
        let path = directory().join("flood.sock");
        let capacity = 8;
        let listener = Listener::bind_with(&path, capacity).unwrap();
        let (address, _) = endpoint::read(&path).unwrap();
        let flood: Vec<TcpStream> = (0..capacity * 5)
            .map(|_| TcpStream::connect(address).unwrap())
            .collect();
        let started = Instant::now();
        ping_through(&path, &listener);
        assert!(started.elapsed() < endpoint::HANDSHAKE_TIMEOUT);
        drop(flood);
    }

    #[test]
    fn a_client_refuses_a_listener_that_does_not_know_the_secret() {
        let path = directory().join("impostor.sock");
        let squatter = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = squatter.local_addr().unwrap().port();
        // The file names a secret the squatter does not have.
        endpoint::publish(&path, port, &[7; TOKEN_BYTES]).unwrap();
        let rogue = thread::spawn(move || {
            let (mut peer, _) = squatter.accept().unwrap();
            let mut nonce = [0; NONCE_BYTES];
            peer.read_exact(&mut nonce).unwrap();
            // A well-formed reply whose proof is made with the wrong secret.
            let (_, reply) = endpoint::answer(&[9; TOKEN_BYTES], &nonce).unwrap();
            peer.write_all(&reply).unwrap();
            // Whatever the client sends after its nonce, until it hangs up.
            let mut rest = Vec::new();
            peer.read_to_end(&mut rest).unwrap();
            rest
        });
        let error = connect(&path).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(rogue.join().unwrap().is_empty());
    }

    #[test]
    fn a_path_that_is_not_an_endpoint_is_left_alone() {
        let path = directory().join("plain.txt");
        fs::write(&path, "not an endpoint").unwrap();
        assert!(matches!(
            Listener::bind(&path),
            Err(BindError::PathNotEndpoint)
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "not an endpoint");
    }
}
