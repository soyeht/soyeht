//! The endpoint file of the loopback flavour of the local endpoint, and the proofs a client and
//! a listener exchange before either trusts the other.
//!
//! The secret never crosses the connection. The client sends a random nonce; the listener
//! answers with its own nonce and an HMAC of both nonces under the secret, which only a process
//! that read the owner-only file can compute; the client checks it and answers with the HMAC for
//! the other direction. A process that merely listens on the port (a leftover of a dead engine,
//! or a squatter) cannot answer, so the client stops after sending its nonce.
//!
//! A live listener holds the lock of its endpoint file (an exclusive lock on one byte far past
//! the end, so the file stays readable), and the system drops the lock when the process ends,
//! however it ends: a crash, a kill or a reboot leave the file but not the lock. A client that
//! can take the lock knows nothing listens and says so at once. Asking the network is not
//! enough: Windows reports a refused loopback connection only after about two seconds, as long
//! as a client can afford to wait, so an engine that is absent looks the same as one that is
//! slow.

#![cfg_attr(not(windows), allow(dead_code))]

use super::BindError;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::{
    collections::VecDeque,
    fs::{self, File},
    io,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream},
    path::Path,
    time::{Duration, Instant},
};

pub(super) const TOKEN_BYTES: usize = 32;
pub(super) const NONCE_BYTES: usize = 32;
pub(super) const PROOF_BYTES: usize = 32;
/// How long a connection has, in all, to finish the exchange before it is dropped.
pub(super) const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);
/// Connections that may be mid-exchange at once. At the limit the oldest one is dropped to make
/// room, so a connection that never answers cannot keep a good client out.
pub(super) const MAX_PENDING: usize = 256;
const MAGIC: &str = "soyeht-local-2";

pub(super) type Token = [u8; TOKEN_BYTES];
pub(super) type Nonce = [u8; NONCE_BYTES];
pub(super) type Proof = [u8; PROOF_BYTES];

/// Whose proof an HMAC is, so one cannot be replayed as the other.
#[derive(Clone, Copy)]
pub(super) enum Side {
    Server,
    Client,
}

impl Side {
    fn label(self) -> &'static [u8] {
        match self {
            Self::Server => b"soyeht-local-2 server",
            Self::Client => b"soyeht-local-2 client",
        }
    }

    fn refusal(self) -> io::Error {
        let message = match self {
            Self::Server => "the endpoint does not know the secret",
            Self::Client => "the client does not know the secret",
        };
        io::Error::new(io::ErrorKind::PermissionDenied, message)
    }
}

pub(super) fn random<const N: usize>() -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
    Ok(bytes)
}

fn mac(token: &Token, side: Side, client: &Nonce, server: &Nonce) -> io::Result<Hmac<Sha256>> {
    let mut mac = Hmac::<Sha256>::new_from_slice(token)
        .map_err(|_| io::Error::other("the secret has an invalid length"))?;
    mac.update(side.label());
    mac.update(client);
    mac.update(server);
    Ok(mac)
}

pub(super) fn proof(
    token: &Token,
    side: Side,
    client: &Nonce,
    server: &Nonce,
) -> io::Result<Proof> {
    Ok(mac(token, side, client, server)?
        .finalize()
        .into_bytes()
        .into())
}

/// Checks `presented` in constant time.
pub(super) fn verify(
    token: &Token,
    side: Side,
    client: &Nonce,
    server: &Nonce,
    presented: &Proof,
) -> io::Result<()> {
    mac(token, side, client, server)?
        .verify_slice(presented)
        .map_err(|_| side.refusal())
}

/// The listener's answer to a client nonce: its own nonce and its proof, ready to send.
pub(super) fn answer(
    token: &Token,
    client: &Nonce,
) -> io::Result<(Nonce, [u8; NONCE_BYTES + PROOF_BYTES])> {
    let server = random::<NONCE_BYTES>()?;
    let mut reply = [0; NONCE_BYTES + PROOF_BYTES];
    reply[..NONCE_BYTES].copy_from_slice(&server);
    reply[NONCE_BYTES..].copy_from_slice(&proof(token, Side::Server, client, &server)?);
    Ok((server, reply))
}

/// Errors of `accept` that concern one connection, not the listener.
pub(super) fn is_transient(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::Interrupted
            | io::ErrorKind::TimedOut
    )
}

/// The connections that are mid-exchange, oldest first.
pub(super) struct Admission<T> {
    capacity: usize,
    next: u64,
    entries: VecDeque<(u64, T)>,
}

impl<T> Admission<T> {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            next: 0,
            entries: VecDeque::new(),
        }
    }

    /// Adds a connection and returns its number. At capacity the oldest one is returned too, to
    /// be closed: a connection that is still waiting after the others have come and gone is the
    /// one least likely to be a client.
    pub(super) fn admit(&mut self, item: T) -> (u64, Option<T>) {
        let evicted = if self.entries.len() >= self.capacity {
            self.entries.pop_front().map(|(_, old)| old)
        } else {
            None
        };
        self.next += 1;
        self.entries.push_back((self.next, item));
        (self.next, evicted)
    }

    /// Forgets a connection whose exchange ended.
    pub(super) fn release(&mut self, number: u64) {
        self.entries.retain(|(entry, _)| *entry != number);
    }

    /// Forgets the connections `keep` rejects. Only the asynchronous flavour sweeps its
    /// finished handshakes this way.
    #[cfg(feature = "tokio")]
    pub(super) fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        self.entries.retain(|(_, item)| keep(item));
    }
}

fn hex(token: &Token) -> String {
    token.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<Token> {
    if text.len() != TOKEN_BYTES * 2 || !text.is_ascii() {
        return None;
    }
    let mut token = [0; TOKEN_BYTES];
    for (slot, pair) in token.iter_mut().zip(text.as_bytes().chunks(2)) {
        let high = char::from(pair[0]).to_digit(16)?;
        let low = char::from(pair[1]).to_digit(16)?;
        // Both digits are below 16, so the byte cannot overflow.
        *slot = (high << 4 | low) as u8;
    }
    Some(token)
}

/// Replaces an endpoint file left by a dead process; refuses any other file.
fn clear_stale(path: &Path) -> Result<(), BindError> {
    super::clear_stale(path, |metadata| {
        metadata.is_file()
            && fs::read_to_string(path).is_ok_and(|text| text.starts_with("soyeht-local-"))
    })
}

/// The lock of a published endpoint file, kept for as long as its listener lives.
#[must_use = "the endpoint reads as stale once this is dropped"]
pub(super) struct Alive(#[allow(dead_code)] File);

/// Writes the endpoint file for a listener on `port`, owner-only, replacing a stale one. The
/// lock is taken before the file appears under its name, so a client that can read the file
/// and take its lock is looking at a listener that is gone. The file holds the secret, so it is
/// owner-only from its first byte ([`crate::fs::WriteMode::OwnerOnly`]).
pub(super) fn publish(path: &Path, port: u16, token: &Token) -> Result<Alive, BindError> {
    clear_stale(path)?;
    let text = format!("{MAGIC} {port} {}\n", hex(token));
    let staged =
        crate::fs::stage_replacement(path, text.as_bytes(), crate::fs::WriteMode::OwnerOnly)?;
    // The staged file has a name of its own, so this lock is never contended: two listeners are
    // kept apart by the engine's `engine.lock` singleton, not here. The lock is taken to be held.
    if !crate::fs::try_lock(staged.file())? {
        return Err(io::Error::new(io::ErrorKind::AddrInUse, "the endpoint is in use").into());
    }
    Ok(Alive(staged.commit()?))
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "not a local endpoint file")
}

/// The port and secret in the text of an endpoint file.
fn parse(text: &str) -> io::Result<(SocketAddrV4, Token)> {
    let mut fields = text.split_whitespace();
    let (Some(MAGIC), Some(port), Some(token), None) =
        (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        return Err(invalid());
    };
    let port = port.parse().map_err(|_| invalid())?;
    let token = unhex(token).ok_or_else(invalid)?;
    Ok((SocketAddrV4::new(Ipv4Addr::LOCALHOST, port), token))
}

/// The port and secret the endpoint file at `path` names. Tests read the file this way.
#[cfg(test)]
pub(super) fn read(path: &Path) -> io::Result<(SocketAddrV4, Token)> {
    parse(&fs::read_to_string(path)?)
}

/// Gives the next read and write on `stream` what is left of `deadline`.
fn within(stream: &TcpStream, deadline: Instant) -> io::Result<()> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(io::ErrorKind::TimedOut.into());
    }
    stream.set_read_timeout(Some(left))?;
    stream.set_write_timeout(Some(left))
}

fn read_array<const N: usize>(mut stream: &TcpStream) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    stream.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// The listener's half of the exchange on a new connection; on success the stream is ready for
/// the caller's own traffic.
pub(super) fn serve(stream: &TcpStream, token: &Token) -> io::Result<()> {
    let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    within(stream, deadline)?;
    let client = read_array::<NONCE_BYTES>(stream)?;
    let (server, reply) = answer(token, &client)?;
    within(stream, deadline)?;
    (&*stream).write_all(&reply)?;
    within(stream, deadline)?;
    let presented = read_array::<PROOF_BYTES>(stream)?;
    verify(token, Side::Client, &client, &server, &presented)?;
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(None)?;
    stream.set_nodelay(true)
}

/// Connects to the endpoint at `path`. The client sends only a nonce until the listener has
/// proved that it knows the secret, and refuses a listener that cannot.
///
/// An endpoint file whose listener is gone is refused at once (`ConnectionRefused`), without
/// asking the network: see the top of this file. A listener that is there but does not answer
/// in time is `TimedOut`.
pub(super) fn connect(path: &Path) -> io::Result<TcpStream> {
    let mut file = File::open(path)?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    let (address, token) = parse(&text)?;
    // A lock that cannot be asked about proves nothing, so the connection decides.
    if crate::fs::try_lock(&file).unwrap_or(false) {
        return Err(io::Error::new(
            io::ErrorKind::ConnectionRefused,
            "nothing listens at the endpoint",
        ));
    }
    drop(file);
    let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    let mut stream = TcpStream::connect_timeout(&SocketAddr::V4(address), HANDSHAKE_TIMEOUT)?;
    stream.set_nodelay(true)?;
    let client = random::<NONCE_BYTES>()?;
    within(&stream, deadline)?;
    stream.write_all(&client)?;
    within(&stream, deadline)?;
    let server = read_array::<NONCE_BYTES>(&stream)?;
    let presented = read_array::<PROOF_BYTES>(&stream)?;
    verify(&token, Side::Server, &client, &server, &presented)?;
    within(&stream, deadline)?;
    stream.write_all(&proof(&token, Side::Client, &client, &server)?)?;
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(None)?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_file_is_owner_only_whatever_a_stale_one_was_and_leaves_nothing_beside_it() {
        let directory = std::env::temp_dir().join(format!(
            "soyeht-endpoint-file-{}-{}",
            std::process::id(),
            random::<4>().map_or(0, u32::from_le_bytes)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("endpoint-file");
        fs::write(&path, format!("{MAGIC} 1 {}\n", hex(&[0; TOKEN_BYTES]))).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        }
        let alive = publish(&path, 4242, &[7; TOKEN_BYTES]).unwrap();
        assert!(crate::fs::file_is_owner_only(&File::open(&path).unwrap()).unwrap());
        assert_eq!(read(&path).unwrap().0.port(), 4242);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        drop(alive);
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_token_survives_the_endpoint_file_and_a_wrong_one_does_not_verify() {
        let token = random::<TOKEN_BYTES>().unwrap();
        assert_eq!(unhex(&hex(&token)), Some(token));
        assert_eq!(unhex("zz"), None);
        let (client, server) = (random().unwrap(), random().unwrap());
        let good = proof(&token, Side::Server, &client, &server).unwrap();
        verify(&token, Side::Server, &client, &server, &good).unwrap();
        let mut other = token;
        other[TOKEN_BYTES - 1] ^= 1;
        assert!(verify(&other, Side::Server, &client, &server, &good).is_err());
        // A proof for one direction is no proof for the other.
        assert!(verify(&token, Side::Client, &client, &server, &good).is_err());
    }

    #[test]
    fn at_capacity_the_oldest_connection_makes_room() {
        let mut admission = Admission::new(2);
        let (first, none) = admission.admit("first");
        assert!(none.is_none());
        let (_, none) = admission.admit("second");
        assert!(none.is_none());
        let (third, evicted) = admission.admit("third");
        assert_eq!(evicted, Some("first"));
        admission.release(first);
        admission.release(third);
        let (_, none) = admission.admit("fourth");
        assert!(none.is_none());
    }
}
