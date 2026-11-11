//! The loopback flavour of the local endpoint, used on Windows. The code does not depend on the
//! system, so its tests run everywhere.

#![cfg_attr(not(windows), allow(dead_code))]

use super::{
    BindError,
    endpoint::{self, MAX_PENDING},
    handshake::Accepting,
};
use std::{
    io,
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::Path,
    time::Duration,
};

pub type Stream = TcpStream;

/// A loopback listener that hands out only the connections that proved they know the secret.
pub struct Listener {
    accepting: Accepting<Stream>,
    /// Marks the endpoint file as live while this listener is.
    _alive: endpoint::Alive,
}

/// A loopback socket on a free port that no other process can bind, whatever it asks for.
pub(super) fn bind_loopback() -> io::Result<TcpListener> {
    crate::net::listen(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
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
        let alive = endpoint::publish(path, address.port(), &token)?;
        let accepting = Accepting::start(
            move || tcp.accept().map(|(stream, _)| stream),
            token,
            capacity,
            move || {
                drop(TcpStream::connect_timeout(
                    &address,
                    Duration::from_millis(200),
                ))
            },
        )?;
        Ok(Self {
            accepting,
            _alive: alive,
        })
    }

    /// Waits for the next connection that proved it knows the secret. Fails with the error
    /// that stopped the listener, and with `BrokenPipe` on every call after that.
    pub fn accept(&self) -> io::Result<Stream> {
        self.accepting.accept()
    }
}

pub fn connect(path: &Path) -> io::Result<Stream> {
    endpoint::connect(path)
}

/// Removes the endpoint file at `path`. A missing one is fine.
pub fn remove(path: &Path) -> io::Result<()> {
    super::remove_if_there(path)
}

/// Whether the peer runs as the same user as this process. Only a process that could read the
/// owner-only endpoint file knows the secret, and [`Listener::accept`] checked it already.
pub fn peer_is_same_user(_: &Stream) -> io::Result<bool> {
    Ok(true)
}

/// Two connected streams, for tests and in-process wiring.
pub fn pair() -> io::Result<(Stream, Stream)> {
    let listener = crate::net::listen(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
    let client = TcpStream::connect(listener.local_addr()?)?;
    let (server, _) = listener.accept()?;
    Ok((client, server))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        local::{
            endpoint::{NONCE_BYTES, TOKEN_BYTES},
            handshake::serve,
            test_directory as directory,
        },
        net::ConnectionErrors,
    };
    use std::{
        fs,
        io::{Read, Write},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::Instant,
    };

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
        crate::local::assert_ping(connect(&path), listener.accept());
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
        crate::local::assert_ping(connect(&path), listener.accept());
        assert!(started.elapsed() < endpoint::HANDSHAKE_TIMEOUT);
        drop(flood);
    }

    #[test]
    fn a_client_refuses_a_listener_that_does_not_know_the_secret() {
        let path = directory().join("impostor.sock");
        let squatter = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = squatter.local_addr().unwrap().port();
        // The file names a secret the squatter does not have.
        let _alive = endpoint::publish(&path, port, &[7; TOKEN_BYTES]).unwrap();
        let rogue = thread::spawn(move || {
            let (mut peer, _) = squatter.accept().unwrap();
            crate::local::impostor_hears(&mut peer)
        });
        crate::local::assert_impostor_refused(connect(&path), rogue);
    }

    /// A loopback port that takes connections and never answers, as an engine that is busy
    /// would, and an endpoint file naming it.
    fn silent_endpoint(name: &str) -> (std::path::PathBuf, TcpListener) {
        let path = directory().join(name);
        let silent = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = silent.local_addr().unwrap().port();
        drop(endpoint::publish(&path, port, &[7; TOKEN_BYTES]).unwrap());
        (path, silent)
    }

    #[test]
    fn an_endpoint_whose_listener_is_gone_is_refused_at_once() {
        let _forks = crate::releasing();
        // Something still accepts connections at the port, so only the dead listener's lock can
        // tell that the file is stale, and no connection is tried (on Windows a refused one
        // takes two seconds to be reported, which is a timeout to a client).
        let (path, _silent) = silent_endpoint("gone.sock");
        let started = Instant::now();
        let error = connect(&path).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::ConnectionRefused);
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn a_listener_that_is_there_but_does_not_answer_is_slow_not_absent() {
        let path = directory().join("slow.sock");
        let silent = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let _alive =
            endpoint::publish(&path, silent.local_addr().unwrap().port(), &[7; 32]).unwrap();
        let error = connect(&path).unwrap_err();
        assert!(
            matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ),
            "{error:?}"
        );
    }

    #[test]
    fn a_listener_that_stops_leaves_a_file_that_reads_as_stale() {
        let _forks = crate::releasing();
        let path = directory().join("stopped.sock");
        let listener = Listener::bind(&path).unwrap();
        crate::local::assert_ping(connect(&path), listener.accept());
        drop(listener);
        assert_eq!(
            connect(&path).unwrap_err().kind(),
            io::ErrorKind::ConnectionRefused
        );
        // The next listener takes the path over and is reachable again.
        let listener = Listener::bind(&path).unwrap();
        crate::local::assert_ping(connect(&path), listener.accept());
    }

    #[test]
    fn a_missing_endpoint_file_is_not_found() {
        let error = connect(&directory().join("never-there.sock")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
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

    /// An accept that fails for a reason that concerns one connection, for ever (a dead network
    /// interface reports it on every call).
    struct EveryAcceptFails;

    impl Iterator for EveryAcceptFails {
        type Item = io::Result<TcpStream>;

        fn next(&mut self) -> Option<Self::Item> {
            Some(Err(io::Error::from(io::ErrorKind::ConnectionAborted)))
        }
    }

    /// What `serve` sends to the one who accepts, when it is fed `incoming` with `errors` and
    /// ended (by `incoming`, or by the stop flag after `patience`).
    fn heard_from_serve(
        incoming: impl Iterator<Item = io::Result<TcpStream>> + Send + 'static,
        limit: Duration,
        patience: Duration,
    ) -> Result<io::Result<Stream>, std::sync::mpsc::TryRecvError> {
        let (sender, receiver) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let token = endpoint::random().unwrap();
        let worker = thread::spawn(move || {
            serve(
                incoming,
                ConnectionErrors::within(limit),
                token,
                &sender,
                &stopping,
                MAX_PENDING,
            );
        });
        let heard = receiver
            .recv_timeout(patience)
            .map_err(|error| match error {
                std::sync::mpsc::RecvTimeoutError::Timeout => std::sync::mpsc::TryRecvError::Empty,
                std::sync::mpsc::RecvTimeoutError::Disconnected => {
                    std::sync::mpsc::TryRecvError::Disconnected
                }
            });
        // A loop that has not ended is stopped, so the test can report it.
        stop.store(true, Ordering::Release);
        worker.join().unwrap();
        heard
    }

    #[test]
    fn a_listener_whose_every_accept_fails_gives_up_and_says_so() {
        let heard = heard_from_serve(
            EveryAcceptFails,
            Duration::from_millis(100),
            Duration::from_secs(5),
        );
        let ended = heard.expect("the listener went on for ever").unwrap_err();
        assert!(crate::net::is_accept_give_up(&ended));
    }

    /// Errors for 300 ms, one connection, errors for 400 ms more. The limit is 500 ms: the two
    /// runs are each under it and together over it, so only a connection that ends the run keeps
    /// the listener.
    #[test]
    fn a_connection_that_is_accepted_ends_the_run_of_errors() {
        let (_client, accepted) = pair().unwrap();
        let mut accepted = Some(accepted);
        let began = std::time::Instant::now();
        let incoming = std::iter::from_fn(move || {
            let elapsed = began.elapsed();
            // The accepts end here, with the second run at 400 ms.
            if elapsed >= Duration::from_millis(700) {
                return None;
            }
            if elapsed >= Duration::from_millis(300)
                && let Some(connection) = accepted.take()
            {
                return Some(Ok(connection));
            }
            Some(Err(io::Error::from(io::ErrorKind::ConnectionAborted)))
        });
        let heard = heard_from_serve(incoming, Duration::from_millis(500), Duration::from_secs(3));
        // The listener did not give up: it only ran out of accepts.
        assert!(
            matches!(heard, Err(std::sync::mpsc::TryRecvError::Disconnected)),
            "the listener gave up: {heard:?}"
        );
    }
}
