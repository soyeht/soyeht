//! A byte stream to the engine. Framing is the shared protocol crate's `Frame`; this module
//! only produces the two halves of a connection and a way to cut it.

use crate::Result;
use soyeht_platform::local;
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

/// Connects to the engine's local endpoint: a Unix socket, or on Windows the loopback port
/// named by the owner-only endpoint file (contract 08, section 2.5).
pub fn connect(path: &Path) -> Result<Link> {
    link(local::connect(path)?)
}

pub fn link(stream: local::Stream) -> Result<Link> {
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
