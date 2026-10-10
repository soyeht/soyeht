use super::BindError;
use std::{
    fs, io,
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::Path,
};

pub type Stream = UnixStream;

/// A listening Unix socket, owner-only.
pub struct Listener(UnixListener);

impl Listener {
    /// Replaces a stale socket at `path` and listens there. Only the owner may connect.
    pub fn bind(path: &Path) -> Result<Self, BindError> {
        super::clear_stale(path, |metadata| metadata.file_type().is_socket())?;
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok(Self(listener))
    }

    pub fn accept(&self) -> io::Result<Stream> {
        self.0.accept().map(|(stream, _)| stream)
    }

    /// The standard listener, for the asynchronous flavour to take over.
    #[cfg(feature = "tokio")]
    pub(super) fn into_std(self) -> UnixListener {
        self.0
    }

    pub fn incoming(&self) -> impl Iterator<Item = io::Result<Stream>> + '_ {
        self.0.incoming()
    }
}

pub fn connect(path: &Path) -> io::Result<Stream> {
    UnixStream::connect(path)
}

/// Two connected streams, for tests and in-process wiring.
pub fn pair() -> io::Result<(Stream, Stream)> {
    UnixStream::pair()
}
