//! The local endpoint: how a process of the same user reaches the engine or the supervisor.
//!
//! The caller names the endpoint with a path inside its private data directory, the same path
//! on every system:
//!
//! - **macOS and Linux**: a Unix socket at that path, owner-only (mode 0600). The kernel
//!   proves who the peer is.
//! - **Windows**: Windows has no Unix sockets in the standard library, so the path is a small
//!   owner-only file naming a loopback port and a random secret. A client reads the file and
//!   connects to `127.0.0.1`; the two sides then prove to each other that they know the secret
//!   (an HMAC over fresh nonces) without ever sending it, and the listener drops every
//!   connection that does not. Only a process that can read the owner-only file can connect,
//!   and a client talks to nothing that cannot prove it is the listener that wrote the file.
//!
//! # Proof
//!
//! A Unix socket is open to every process of the user that can reach its directory, and what
//! listens on it may start programs and hold terminals (the supervisor). Such a listener binds a
//! [`ProvedListener`] and its clients use [`connect_proved`]: a secret that only a process able to
//! read an owner-only file knows, and the exchange of Windows over the socket (a nonce each way
//! and an HMAC under the secret, the secret never sent). The listener hands out only the
//! connections that proved it, before it reads a byte of its own protocol; the client
//! refuses a listener that cannot. An endpoint that does not ask (the engine's, which the
//! apps and the command line reach with the kernel's word) uses [`Listener`] and [`connect`].
//!
//! A [`Stream`] has the methods of a standard socket (`try_clone`, `shutdown`, read and write
//! timeouts, `Read` and `Write`). The synchronous flavour is here; the `tokio` feature adds
//! [`asynchronous`].

use std::{fmt, fs, io, path::Path};

#[cfg(feature = "tokio")]
pub mod asynchronous;
mod endpoint;
mod handshake;
#[cfg(any(windows, test))]
mod tcp;
#[cfg(unix)]
mod unix;

#[cfg(windows)]
pub use tcp::{Listener, Stream, connect, pair, peer_is_same_user};
/// The listener and the connection of an endpoint that asks for the proof; see "Proof" above. On
/// Windows every endpoint asks, so these are `Listener` and `connect`.
#[cfg(windows)]
pub use tcp::{Listener as ProvedListener, connect as connect_proved, remove as remove_proved};
#[cfg(unix)]
pub use unix::{Listener, Stream, connect, pair, peer_is_same_user};
#[cfg(unix)]
pub use unix::{ProvedListener, connect_proved, remove_proved};

/// Why an endpoint could not be created.
#[derive(Debug)]
pub enum BindError {
    /// The path exists and is not an endpoint this crate created, so it is left alone.
    PathNotEndpoint,
    Io(io::Error),
}

impl From<io::Error> for BindError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl fmt::Display for BindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PathNotEndpoint => formatter.write_str("endpoint path is not an endpoint"),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for BindError {}

/// Removes what a dead process left at `path` when `ours` says it is an endpoint; anything else
/// at the path is refused and left alone, and a missing path is fine.
fn clear_stale(path: &Path, ours: impl FnOnce(&fs::Metadata) -> bool) -> Result<(), BindError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if ours(&metadata) => Ok(fs::remove_file(path)?),
        Ok(_) => Err(BindError::PathNotEndpoint),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Removes the file at `path`; a missing one is fine.
fn remove_if_there(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// A client connected, says "ping", and the listener's side of it hears exactly that.
#[cfg(test)]
fn assert_ping(client: io::Result<impl std::io::Write>, server: io::Result<impl std::io::Read>) {
    let (mut client, mut server) = (client.unwrap(), server.unwrap());
    client.write_all(b"ping").unwrap();
    let mut received = [0; 4];
    server.read_exact(&mut received).unwrap();
    assert_eq!(&received, b"ping");
}

/// What a listener that does not know the secret does with one connection: it answers the
/// client's nonce with a well-formed reply whose proof is made under another secret, and returns
/// what the client sends after that until it hangs up.
#[cfg(test)]
fn impostor_hears(peer: &mut (impl std::io::Read + std::io::Write)) -> Vec<u8> {
    let mut nonce = [0; endpoint::NONCE_BYTES];
    peer.read_exact(&mut nonce).unwrap();
    let (_, reply) = endpoint::answer(&[9; endpoint::TOKEN_BYTES], &nonce).unwrap();
    peer.write_all(&reply).unwrap();
    let mut rest = Vec::new();
    peer.read_to_end(&mut rest).unwrap();
    rest
}

/// The client of a listener that does not know the secret refused it (`connected` is its attempt)
/// and sent it nothing after its nonce (`heard` is what the listener's thread returned).
#[cfg(test)]
fn assert_impostor_refused(
    connected: io::Result<impl fmt::Debug>,
    heard: std::thread::JoinHandle<Vec<u8>>,
) {
    assert_eq!(
        connected.unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert!(heard.join().unwrap().is_empty());
}

/// A folder for the endpoint files of the tests of this process.
#[cfg(test)]
fn test_directory() -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!("soyeht-local-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    directory
}
