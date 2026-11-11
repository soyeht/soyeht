use super::{
    BindError,
    endpoint::{self, HANDSHAKE_TIMEOUT, MAX_PENDING},
    handshake::Accepting,
};
use std::{
    fs, io,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    time::Instant,
};

pub type Stream = UnixStream;

/// A listening Unix socket, owner-only.
pub struct Listener(UnixListener);

/// Replaces a stale socket at `path` and listens there, owner-only.
fn bind_socket(path: &Path) -> Result<UnixListener, BindError> {
    super::clear_stale(path, |metadata| metadata.file_type().is_socket())?;
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

impl Listener {
    /// Replaces a stale socket at `path` and listens there. Only the owner may connect.
    pub fn bind(path: &Path) -> Result<Self, BindError> {
        Ok(Self(bind_socket(path)?))
    }

    pub fn accept(&self) -> io::Result<Stream> {
        self.0.accept().map(|(stream, _)| stream)
    }

    /// The standard listener, for the asynchronous flavour to take over.
    #[cfg(feature = "tokio")]
    pub(super) fn into_std(self) -> UnixListener {
        self.0
    }
}

pub fn connect(path: &Path) -> io::Result<Stream> {
    UnixStream::connect(path)
}

/// Where the secret of the proved socket at `path` is kept: beside it, with `.secret` added.
fn secret_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".secret");
    PathBuf::from(name)
}

/// A listening Unix socket whose connections must prove that they know a secret before anyone
/// accepts them: the same exchange the loopback endpoint of Windows runs, on a socket. The kernel
/// already says who the peer is; this keeps out a process of the same user that cannot read the
/// secret file, which a supervisor that starts programs and holds terminals needs (ADR 0160).
pub struct ProvedListener {
    accepting: Accepting<Stream>,
}

impl ProvedListener {
    /// Writes a new owner-only secret file beside `path`, replacing the one a listener that is
    /// gone left, then replaces a stale socket at `path` and listens there. The secret is
    /// written first, so a client that can connect finds the secret of this listener and not the
    /// one before it; the other order leaves a moment in which it finds the old one.
    pub fn bind(path: &Path) -> Result<Self, BindError> {
        let token = endpoint::random()?;
        endpoint::write_secret(&secret_path(path), &token)?;
        let listener = bind_socket(path)?;
        let waking = path.to_owned();
        let accepting = Accepting::start(
            move || listener.accept().map(|(stream, _)| stream),
            token,
            MAX_PENDING,
            move || drop(UnixStream::connect(&waking)),
        )?;
        Ok(Self { accepting })
    }

    /// Waits for the next connection that proved it knows the secret. Fails with the error
    /// that stopped the listener, and with `BrokenPipe` on every call after that.
    pub fn accept(&self) -> io::Result<Stream> {
        self.accepting.accept()
    }
}

/// Connects to the proved socket at `path`: reads the secret beside it, connects, and proves to
/// the listener that it knows the secret, and checks the listener's proof in turn. A listener that
/// cannot prove it is refused (`PermissionDenied`) after the client has sent only a nonce.
///
/// The connection comes first, so that a client meeting a supervisor that has just restarted reads
/// the secret that the new one wrote before it bound its socket. A socket with no secret file
/// beside it is a supervisor from before the proof, and is refused (`InvalidData`), not read as
/// an absent one: it must not be started a second time.
pub fn connect_proved(path: &Path) -> io::Result<Stream> {
    let stream = UnixStream::connect(path)?;
    let token = endpoint::read_secret(&secret_path(path)).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "the endpoint has no secret file",
            )
        } else {
            error
        }
    })?;
    endpoint::prove(&stream, &token, Instant::now() + HANDSHAKE_TIMEOUT)?;
    Ok(stream)
}

/// Removes the socket at `path` and the secret file beside it. A missing one is fine.
pub fn remove_proved(path: &Path) -> io::Result<()> {
    let socket = super::remove_if_there(path);
    let secret = super::remove_if_there(&secret_path(path));
    socket.and(secret)
}

/// Two connected streams, for tests and in-process wiring.
pub fn pair() -> io::Result<(Stream, Stream)> {
    UnixStream::pair()
}

/// The user id of the process at the other end of the socket `descriptor`, which the kernel
/// reports. One implementation for the standard and the asynchronous stream.
#[cfg(target_os = "macos")]
pub(super) fn peer_user_of(descriptor: std::os::fd::RawFd) -> io::Result<u32> {
    let (mut user, mut group) = (0, 0);
    // SAFETY: both out pointers are valid for the call.
    let status = unsafe { libc::getpeereid(descriptor, &raw mut user, &raw mut group) };
    if status == 0 {
        Ok(user)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn peer_user_of(descriptor: std::os::fd::RawFd) -> io::Result<u32> {
    let mut credential = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `credential` has the size passed in `length`.
    let status = unsafe {
        libc::getsockopt(
            descriptor,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credential).cast(),
            &raw mut length,
        )
    };
    if status == 0 {
        Ok(credential.uid)
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Whether the peer of `stream` runs as the same user as this process, or why the kernel could not
/// say. The socket's mode and its private directory already keep other users out; this is the check
/// that does not depend on them.
pub fn peer_is_same_user(stream: &Stream) -> io::Result<bool> {
    descriptor_peer_is_same_user(stream.as_raw_fd())
}

/// [`peer_is_same_user`] for the socket `descriptor`: the one comparison of the peer's user with
/// this process's, for the standard and the asynchronous stream.
pub(super) fn descriptor_peer_is_same_user(descriptor: std::os::fd::RawFd) -> io::Result<bool> {
    Ok(peer_user_of(descriptor)? == crate::user::effective_id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_peer_of_a_socket_pair_runs_as_this_user_and_a_lookup_that_fails_is_an_error() {
        let (left, _right) = pair().unwrap();
        let me = crate::user::effective_id();
        assert!(peer_is_same_user(&left).unwrap());
        assert_eq!(peer_user_of(left.as_raw_fd()).unwrap(), me);
        // A descriptor that is no socket is not "another user": the lookup itself failed.
        assert!(peer_user_of(-1).is_err());
    }

    use crate::local::{
        endpoint::{NONCE_BYTES, PROOF_BYTES, TOKEN_BYTES},
        test_directory as directory,
    };
    use std::io::{Read, Write};

    #[test]
    fn a_client_with_the_secret_is_accepted_and_one_without_never_reaches_accept() {
        let path = directory().join("p1.sock");
        let listener = ProvedListener::bind(&path).unwrap();
        // The socket itself is open to the user; the proof is what keeps a caller out. This one
        // says a hello, as the supervisor's protocol begins, and then tries a wrong proof.
        let mut intruder = connect(&path).unwrap();
        intruder.write_all(b"\x00\x00\x00\x02{}").unwrap();
        let mut wrong = connect(&path).unwrap();
        wrong.write_all(&[0; NONCE_BYTES]).unwrap();
        let mut reply = [0; NONCE_BYTES + PROOF_BYTES];
        wrong.read_exact(&mut reply).unwrap();
        wrong.write_all(&[0; PROOF_BYTES]).unwrap();
        // Neither is in the way of the one that knows the secret, and the listener hands out
        // only that one: the next connection it yields is the client's, with its bytes.
        crate::local::assert_ping(connect_proved(&path), listener.accept());
        let mut closed = [0; 1];
        assert_eq!(wrong.read(&mut closed).unwrap_or(0), 0);
        assert_eq!(intruder.read(&mut closed).unwrap_or(0), 0);
    }

    #[test]
    fn a_client_refuses_a_listener_that_does_not_know_the_secret() {
        let path = directory().join("p2.sock");
        write_secret_for_test(&path, [7; TOKEN_BYTES]);
        let squatter = UnixListener::bind(&path).unwrap();
        let rogue = std::thread::spawn(move || {
            let (mut peer, _) = squatter.accept().unwrap();
            crate::local::impostor_hears(&mut peer)
        });
        crate::local::assert_impostor_refused(connect_proved(&path), rogue);
    }

    fn write_secret_for_test(socket: &Path, token: endpoint::Token) {
        endpoint::write_secret(&secret_path(socket), &token).unwrap();
    }

    #[test]
    fn the_secret_file_is_owner_only_and_a_restart_replaces_it() {
        let path = directory().join("p3.sock");
        let first = ProvedListener::bind(&path).unwrap();
        let secret = secret_path(&path);
        let before = endpoint::read_secret(&secret).unwrap();
        assert_eq!(
            fs::metadata(&secret).unwrap().permissions().mode() & 0o777,
            0o600
        );
        crate::local::assert_ping(connect_proved(&path), first.accept());
        drop(first);
        // The next listener takes the path over with a secret of its own, and the client reads it
        // afresh each time, so it connects to the new one without being told.
        let second = ProvedListener::bind(&path).unwrap();
        assert_ne!(endpoint::read_secret(&secret).unwrap(), before);
        crate::local::assert_ping(connect_proved(&path), second.accept());
    }

    #[test]
    fn a_secret_file_that_others_can_read_is_not_trusted() {
        let path = directory().join("p4.sock");
        let _listener = ProvedListener::bind(&path).unwrap();
        let secret = secret_path(&path);
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            connect_proved(&path).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn a_socket_with_no_secret_file_is_not_read_as_an_absent_one() {
        let path = directory().join("p5.sock");
        let _plain = Listener::bind(&path).unwrap();
        // A supervisor from before the proof: it is there, so it must not be started again.
        assert_eq!(
            connect_proved(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        // No socket at all is absent, as for `connect`.
        let error = connect_proved(&directory().join("never.sock")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn removing_a_proved_socket_removes_its_secret_and_a_missing_one_is_fine() {
        let path = directory().join("p6.sock");
        drop(ProvedListener::bind(&path).unwrap());
        assert!(path.exists() && secret_path(&path).exists());
        remove_proved(&path).unwrap();
        assert!(!path.exists() && !secret_path(&path).exists());
        remove_proved(&path).unwrap();
    }

    #[test]
    fn a_path_that_is_not_a_socket_is_left_alone() {
        let path = directory().join("p7.txt");
        fs::write(&path, "not an endpoint").unwrap();
        assert!(matches!(
            ProvedListener::bind(&path),
            Err(BindError::PathNotEndpoint)
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "not an endpoint");
    }
}
