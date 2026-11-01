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
    // SAFETY: geteuid has no arguments or memory preconditions.
    Ok(peer_user_of(stream.as_raw_fd())? == unsafe { libc::geteuid() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_peer_of_a_socket_pair_runs_as_this_user_and_a_lookup_that_fails_is_an_error() {
        let (left, _right) = pair().unwrap();
        // SAFETY: geteuid has no arguments or memory preconditions.
        let me = unsafe { libc::geteuid() };
        assert!(peer_is_same_user(&left).unwrap());
        assert_eq!(peer_user_of(left.as_raw_fd()).unwrap(), me);
        // A descriptor that is no socket is not "another user": the lookup itself failed.
        assert!(peer_user_of(-1).is_err());
    }
}
