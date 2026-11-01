use crate::local::BindError;
use std::{io, path::Path};
use tokio::net::{UnixListener, UnixStream};

pub use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

pub type Stream = UnixStream;

/// A listening Unix socket, owner-only.
pub struct Listener(UnixListener);

impl Listener {
    /// Replaces a stale socket at `path` and listens there. Only the owner may connect.
    pub fn bind(path: &Path) -> Result<Self, BindError> {
        let std_listener = crate::local::Listener::bind(path)?.into_std();
        std_listener.set_nonblocking(true)?;
        Ok(Self(UnixListener::from_std(std_listener)?))
    }

    /// Waits for the next connection.
    pub async fn accept(&mut self) -> io::Result<Stream> {
        self.0.accept().await.map(|(stream, _)| stream)
    }
}

/// Whether the peer of `stream` runs as the same user as this process; a peer the kernel cannot
/// name is not.
pub fn same_user(stream: &Stream) -> bool {
    use std::os::fd::AsRawFd;
    // SAFETY: geteuid has no arguments or memory preconditions.
    crate::local::unix::peer_user_of(stream.as_raw_fd())
        .is_ok_and(|user| user == unsafe { libc::geteuid() })
}

/// Two connected streams, for tests and in-process wiring.
pub fn pair() -> io::Result<(Stream, Stream)> {
    UnixStream::pair()
}
