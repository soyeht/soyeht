//! A byte stream to the engine. Framing is the shared protocol crate's `Frame`; this module
//! only produces the two halves of a connection and a way to cut it.

use crate::Result;
use std::{
    io::{Read, Write},
    path::Path,
    sync::Arc,
};

/// Closing wakes any reader blocked on the connection.
pub type Closer = Arc<dyn Fn() + Send + Sync>;

pub struct Link {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub close: Closer,
}

/// Connects to the engine's local socket.
#[cfg(unix)]
pub fn connect(path: &Path) -> Result<Link> {
    link(std::os::unix::net::UnixStream::connect(path)?)
}

/// The engine port defines the Windows local transport; until it lands there is none.
#[cfg(not(unix))]
pub fn connect(_path: &Path) -> Result<Link> {
    Err(crate::Error::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "the engine has no Windows local transport yet",
    )))
}

#[cfg(unix)]
pub fn link(stream: std::os::unix::net::UnixStream) -> Result<Link> {
    let reader = stream.try_clone()?;
    let writer = stream.try_clone()?;
    Ok(Link {
        reader: Box::new(reader),
        writer: Box::new(writer),
        close: Arc::new(move || {
            // A socket that is already closed has nothing left to shut down.
            if let Err(error) = stream.shutdown(std::net::Shutdown::Both)
                && error.kind() != std::io::ErrorKind::NotConnected
            {
                eprintln!("soyeht: could not close the engine connection: {error}");
            }
        }),
    })
}
