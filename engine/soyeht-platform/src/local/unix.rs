use super::BindError;
use std::{
    fs, io,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
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

/// The user id of the process at the other end of `stream`, which the kernel reports.
#[cfg(target_os = "macos")]
fn peer_user(stream: &Stream) -> Option<u32> {
    let (mut user, mut group) = (0, 0);
    // SAFETY: both out pointers are valid for the call; the descriptor is open.
    let status = unsafe { libc::getpeereid(stream.as_raw_fd(), &raw mut user, &raw mut group) };
    (status == 0).then_some(user)
}

#[cfg(not(target_os = "macos"))]
fn peer_user(stream: &Stream) -> Option<u32> {
    let mut credential = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `credential` has the size passed in `length`; the descriptor is open.
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credential).cast(),
            &raw mut length,
        )
    };
    (status == 0).then_some(credential.uid)
}

/// Whether the peer of `stream` runs as the same user as this process. The socket's mode and
/// its private directory already keep other users out; this is the check that does not depend on
/// them.
pub fn same_user(stream: &Stream) -> bool {
    // SAFETY: geteuid has no arguments or memory preconditions.
    peer_runs_as(stream, unsafe { libc::geteuid() })
}

fn peer_runs_as(stream: &Stream, user: u32) -> bool {
    peer_user(stream) == Some(user)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_peer_of_a_socket_pair_runs_as_this_user_and_as_no_other() {
        let (left, _right) = pair().unwrap();
        // SAFETY: geteuid has no arguments or memory preconditions.
        let me = unsafe { libc::geteuid() };
        assert!(same_user(&left));
        assert!(peer_runs_as(&left, me));
        assert!(!peer_runs_as(&left, me.wrapping_add(1)));
    }
}
