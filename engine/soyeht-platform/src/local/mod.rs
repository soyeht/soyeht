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
//! A [`Stream`] has the methods of a standard socket (`try_clone`, `shutdown`, read and write
//! timeouts, `Read` and `Write`). The synchronous flavour is here; the `tokio` feature adds
//! [`asynchronous`].

use std::{fmt, fs, io, path::Path};

#[cfg(feature = "tokio")]
pub mod asynchronous;
#[cfg(any(windows, test))]
mod endpoint;
#[cfg(windows)]
mod exclusive;
#[cfg(any(windows, test))]
mod tcp;
#[cfg(unix)]
mod unix;

#[cfg(windows)]
pub use tcp::{Listener, Stream, connect, pair, peer_is_same_user};
#[cfg(unix)]
pub use unix::{Listener, Stream, connect, pair, peer_is_same_user};

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

/// A folder for the endpoint files of the tests of this process.
#[cfg(test)]
fn test_directory() -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!("soyeht-local-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    directory
}
