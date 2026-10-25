//! The tokio flavour of the loopback endpoint; see [`crate::local`] for what it is.

#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

use crate::local::{
    BindError,
    endpoint::{
        self, Admission, HANDSHAKE_TIMEOUT, MAX_PENDING, NONCE_BYTES, PROOF_BYTES, Side, Token,
    },
};
use std::{io, path::Path};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::{AbortHandle, JoinSet},
};

pub use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

pub type Stream = TcpStream;

/// A loopback listener that hands out only the connections that proved they know the secret.
pub struct Listener {
    tcp: TcpListener,
    token: Token,
    handshakes: JoinSet<io::Result<Stream>>,
    pending: Admission<AbortHandle>,
    /// Marks the endpoint file as live while this listener is.
    _alive: endpoint::Alive,
}

impl Listener {
    /// Listens on a free loopback port and writes the owner-only endpoint file at `path`.
    /// Call it inside a tokio runtime.
    pub fn bind(path: &Path) -> Result<Self, BindError> {
        Self::bind_with(path, MAX_PENDING)
    }

    fn bind_with(path: &Path, capacity: usize) -> Result<Self, BindError> {
        let std_listener = crate::local::tcp::bind_loopback()?;
        std_listener.set_nonblocking(true)?;
        let port = std_listener.local_addr()?.port();
        let token = endpoint::random()?;
        let alive = endpoint::publish(path, port, &token)?;
        Ok(Self {
            tcp: TcpListener::from_std(std_listener)?,
            token,
            handshakes: JoinSet::new(),
            pending: Admission::new(capacity),
            _alive: alive,
        })
    }

    /// Waits for the next connection that proved it knows the secret. Other connections are
    /// checked at the same time, so a slow one never delays a good one, and when too many wait
    /// the oldest is dropped to make room for the newest. Fails when the listener is dead.
    pub async fn accept(&mut self) -> io::Result<Stream> {
        loop {
            tokio::select! {
                accepted = self.tcp.accept() => {
                    match accepted {
                        Ok((stream, _)) => self.admit(stream),
                        Err(error) if endpoint::is_transient(&error) => {}
                        Err(error) => return Err(error),
                    }
                }
                Some(done) = self.handshakes.join_next(), if !self.handshakes.is_empty() => {
                    if let Ok(Ok(stream)) = done {
                        return Ok(stream);
                    }
                }
            }
        }
    }

    fn admit(&mut self, stream: Stream) {
        self.pending.retain(|handshake| !handshake.is_finished());
        let handle = self.handshakes.spawn(handshake(stream, self.token));
        if let (_, Some(oldest)) = self.pending.admit(handle) {
            oldest.abort();
        }
    }
}

async fn exchange(stream: &mut Stream, token: &Token) -> io::Result<()> {
    let mut client = [0; NONCE_BYTES];
    stream.read_exact(&mut client).await?;
    let (server, reply) = endpoint::answer(token, &client)?;
    stream.write_all(&reply).await?;
    let mut presented = [0; PROOF_BYTES];
    stream.read_exact(&mut presented).await?;
    endpoint::verify(token, Side::Client, &client, &server, &presented)
}

async fn handshake(mut stream: Stream, token: Token) -> io::Result<Stream> {
    tokio::time::timeout(HANDSHAKE_TIMEOUT, exchange(&mut stream, &token))
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
    stream.set_nodelay(true)?;
    Ok(stream)
}

/// Whether the peer runs as the same user as this process. Only a process that could read the
/// owner-only endpoint file knows the secret, and [`Listener::accept`] checked it already.
pub fn same_user(_: &Stream) -> bool {
    true
}

/// Two connected streams, for tests and in-process wiring. Call it inside a tokio runtime.
pub fn pair() -> io::Result<(Stream, Stream)> {
    let (client, server) = crate::local::tcp::pair()?;
    client.set_nonblocking(true)?;
    server.set_nonblocking(true)?;
    Ok((TcpStream::from_std(client)?, TcpStream::from_std(server)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::test_directory as directory;
    use std::time::Instant;

    async fn client(path: &Path) -> io::Result<std::net::TcpStream> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || endpoint::connect(&path)).await?
    }

    /// A client connects, says "ping", and the listener's side of it hears exactly that.
    async fn ping_through(path: &Path, listener: &mut Listener) {
        let (accepted, guest) = tokio::join!(listener.accept(), client(path));
        let mut accepted = accepted.unwrap();
        std::io::Write::write_all(&mut guest.unwrap(), b"ping").unwrap();
        let mut received = [0; 4];
        accepted.read_exact(&mut received).await.unwrap();
        assert_eq!(&received, b"ping");
    }

    #[tokio::test]
    async fn a_good_client_is_accepted_after_a_flood_of_silent_connections() {
        let path = directory().join("async-flood.sock");
        let capacity = 8;
        let mut listener = Listener::bind_with(&path, capacity).unwrap();
        let (address, _) = endpoint::read(&path).unwrap();
        let mut flood = Vec::new();
        for _ in 0..capacity * 5 {
            flood.push(TcpStream::connect(address).await.unwrap());
        }
        let started = Instant::now();
        ping_through(&path, &mut listener).await;
        assert!(started.elapsed() < HANDSHAKE_TIMEOUT);
        drop(flood);
    }

    #[tokio::test]
    async fn a_client_without_the_secret_is_not_accepted() {
        let path = directory().join("async-intruder.sock");
        let mut listener = Listener::bind(&path).unwrap();
        let (address, _) = endpoint::read(&path).unwrap();
        let intruder = tokio::spawn(async move {
            let mut stream = TcpStream::connect(address).await.unwrap();
            stream.write_all(&[0; NONCE_BYTES]).await.unwrap();
            let mut reply = [0; NONCE_BYTES + PROOF_BYTES];
            stream.read_exact(&mut reply).await.unwrap();
            stream.write_all(&[0; PROOF_BYTES]).await.unwrap();
            stream
        });
        ping_through(&path, &mut listener).await;
        drop(intruder.await.unwrap());
    }
}
