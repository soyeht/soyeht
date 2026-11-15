//! The local API of the Tailscale daemon, which tells who a peer address is (ADR 0164).
//!
//! The daemon listens on a Unix socket that any local account may read from on Linux. A request
//! is plain HTTP over it. This module only carries a `GET` and returns the body: what the answer
//! means is the caller's. It never blocks longer than [`DEADLINE`] (connecting included), and an
//! answer that does not fit [`MAX_BODY`] is an error. The answer is believed only when the socket
//! and its folder belong to the account the caller trusts (root, for the system's daemon) and,
//! on Linux, the process at the other end of the connection runs as that account, so a program of
//! another account that put a socket of its own in the daemon's place is never asked.
//! Only Linux has the socket at a known place: elsewhere [`get`] answers
//! [`io::ErrorKind::Unsupported`].

use std::{io, path::Path, time::Duration};

/// Where the Linux daemon listens.
pub const LINUX_SOCKET: &str = "/var/run/tailscale/tailscaled.sock";

/// How long a request may take in all.
pub const DEADLINE: Duration = Duration::from_secs(3);

/// The most a body may hold.
pub const MAX_BODY: usize = 1 << 20;

/// `GET target` (a path that starts with `/localapi/v0/`) on the system's daemon socket, and the
/// body of a `200` answer.
///
/// # Errors
/// [`io::ErrorKind::Unsupported`] where the socket's place is not known, the connection's own
/// error (`NotFound` or `ConnectionRefused` when no daemon runs, `TimedOut` when it does not
/// accept in time), `PermissionDenied` when the account may not use the socket or the socket
/// does not belong to root, `InvalidInput` for a `target` that is not a plain local API path,
/// and `InvalidData` for an answer that is not a `200` with a body that fits.
#[cfg(target_os = "linux")]
pub fn get(target: &str) -> io::Result<Vec<u8>> {
    /// The account that owns the system's daemon.
    const ROOT: u32 = 0;
    get_at(Path::new(LINUX_SOCKET), target, ROOT)
}

/// See the Linux form.
#[cfg(not(target_os = "linux"))]
pub fn get(_target: &str) -> io::Result<Vec<u8>> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// [`get`] on the daemon socket at `socket`, which `trusted` (a user id) must own, as must its
/// folder.
#[cfg(unix)]
pub fn get_at(socket: &Path, target: &str, trusted: u32) -> io::Result<Vec<u8>> {
    if !target.starts_with("/localapi/v0/") || target.bytes().any(|byte| byte <= b' ') {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    unix::get(socket, target, trusted)
}

/// See the Unix form.
#[cfg(not(unix))]
pub fn get_at(_socket: &Path, _target: &str, _trusted: u32) -> io::Result<Vec<u8>> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// The body of an HTTP/1 answer, when it is a `200` whose body is not chunked.
#[cfg(any(unix, test))]
fn body_of(answer: &[u8]) -> io::Result<&[u8]> {
    let invalid = || io::Error::from(io::ErrorKind::InvalidData);
    let split = answer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(invalid)?;
    let (head, body) = (&answer[..split], &answer[split + 4..]);
    let head = std::str::from_utf8(head).map_err(|_| invalid())?;
    let mut lines = head.split("\r\n");
    let status = lines.next().ok_or_else(invalid)?;
    let mut parts = status.split(' ');
    let version = parts.next().unwrap_or_default();
    let code = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/1.") {
        return Err(invalid());
    }
    match code {
        "200" => {}
        "403" => return Err(io::Error::from(io::ErrorKind::PermissionDenied)),
        "404" => return Err(io::Error::from(io::ErrorKind::NotFound)),
        _ => return Err(invalid()),
    }
    let chunked = lines.any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("transfer-encoding")
                && value.to_ascii_lowercase().contains("chunked")
        })
    });
    if chunked || body.len() > MAX_BODY {
        return Err(invalid());
    }
    Ok(body)
}

#[cfg(unix)]
mod unix {
    use super::{DEADLINE, MAX_BODY, body_of};
    use std::{
        io::{self, Read as _, Write as _},
        mem::size_of,
        os::{
            fd::{AsRawFd as _, FromRawFd as _, OwnedFd},
            unix::{
                ffi::OsStrExt as _,
                fs::{FileTypeExt as _, MetadataExt as _},
                net::UnixStream,
            },
        },
        path::Path,
        time::{Duration, Instant},
    };

    /// What the daemon expects in `Host` on its socket.
    const HOST: &str = "local-tailscaled.sock";

    /// How long to wait before asking a daemon whose queue was full again.
    const QUEUE_RETRY: Duration = Duration::from_millis(10);

    pub(super) fn get(socket: &Path, target: &str, trusted: u32) -> io::Result<Vec<u8>> {
        let started = Instant::now();
        check_owner(socket, trusted)?;
        let mut stream = connect(socket, started)?;
        #[cfg(target_os = "linux")]
        check_peer(&stream, trusted)?;
        stream.set_write_timeout(Some(DEADLINE))?;
        // HTTP/1.0: the daemon then closes the connection after the body and never chunks it.
        write!(
            stream,
            "GET {target} HTTP/1.0\r\nHost: {HOST}\r\nSec-Tailscale: localapi\r\n\r\n"
        )?;
        // One timeout for each read, and the whole request is checked against the deadline after
        // each read, so a daemon that sends a byte at a time is not waited for longer either.
        stream.set_read_timeout(Some(DEADLINE))?;
        let mut answer = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let read = match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    return Err(io::Error::from(io::ErrorKind::TimedOut));
                }
                Err(error) => return Err(error),
            };
            answer.extend_from_slice(&buffer[..read]);
            if answer.len() > MAX_BODY + buffer.len() {
                return Err(io::Error::from(io::ErrorKind::InvalidData));
            }
            if started.elapsed() >= DEADLINE {
                return Err(io::Error::from(io::ErrorKind::TimedOut));
            }
        }
        body_of(&answer).map(<[u8]>::to_vec)
    }

    /// The socket is a socket of `trusted`, in a folder of `trusted` that no one else can write.
    fn check_owner(socket: &Path, trusted: u32) -> io::Result<()> {
        let refused = || io::Error::from(io::ErrorKind::PermissionDenied);
        let file = std::fs::metadata(socket)?;
        if !file.file_type().is_socket() || file.uid() != trusted {
            return Err(refused());
        }
        let folder = std::fs::metadata(socket.parent().ok_or_else(refused)?)?;
        if folder.uid() != trusted || folder.mode() & 0o022 != 0 {
            return Err(refused());
        }
        Ok(())
    }

    /// The process at the other end of `stream` runs as `trusted`.
    #[cfg(target_os = "linux")]
    fn check_peer(stream: &UnixStream, trusted: u32) -> io::Result<()> {
        let mut credentials = libc::ucred {
            pid: 0,
            uid: u32::MAX,
            gid: u32::MAX,
        };
        let mut length = size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: the pointers describe `credentials` and `length`, which getsockopt writes.
        let read = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                std::ptr::from_mut(&mut credentials).cast(),
                &mut length,
            )
        };
        if read != 0 {
            return Err(io::Error::last_os_error());
        }
        if credentials.uid == trusted {
            Ok(())
        } else {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        }
    }

    /// Connects to `socket` without waiting longer than the rest of [`DEADLINE`] since `started`:
    /// a daemon that does not accept (its queue is full, or it is stopped) is a timeout.
    fn connect(socket: &Path, started: Instant) -> io::Result<UnixStream> {
        // SAFETY: an all-zero sockaddr_un is a valid value of the type.
        let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        let path = socket.as_os_str().as_bytes();
        if path.is_empty() || path.len() >= address.sun_path.len() || path.contains(&0) {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        address.sun_family = libc::AF_UNIX as libc::sa_family_t;
        for (slot, byte) in address.sun_path.iter_mut().zip(path) {
            *slot = *byte as libc::c_char;
        }
        // SAFETY: socket has no pointer arguments.
        let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: raw is a new descriptor that nothing else owns.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        set_flags(&fd, true)?;
        loop {
            // SAFETY: the pointer and length describe `address`, which connect only reads.
            let connected = unsafe {
                libc::connect(
                    fd.as_raw_fd(),
                    std::ptr::from_ref(&address).cast(),
                    size_of::<libc::sockaddr_un>() as libc::socklen_t,
                )
            };
            if connected == 0 {
                break;
            }
            let error = io::Error::last_os_error();
            match error.raw_os_error() {
                Some(libc::EINTR) => {}
                Some(libc::EAGAIN) | Some(libc::EINPROGRESS) => {
                    if started.elapsed() >= DEADLINE {
                        return Err(io::Error::from(io::ErrorKind::TimedOut));
                    }
                    std::thread::sleep(QUEUE_RETRY);
                }
                _ => return Err(error),
            }
        }
        set_flags(&fd, false)?;
        Ok(UnixStream::from(fd))
    }

    /// Makes the descriptor close on exec, and non-blocking or blocking.
    fn set_flags(fd: &OwnedFd, nonblocking: bool) -> io::Result<()> {
        // SAFETY: fcntl with these commands takes and returns plain integers.
        let (status, descriptor) = unsafe {
            (
                libc::fcntl(fd.as_raw_fd(), libc::F_GETFL),
                libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC),
            )
        };
        if status < 0 || descriptor < 0 {
            return Err(io::Error::last_os_error());
        }
        let wanted = if nonblocking {
            status | libc::O_NONBLOCK
        } else {
            status & !libc::O_NONBLOCK
        };
        // SAFETY: as above.
        if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, wanted) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_two_hundred_answer_gives_its_body_and_nothing_else_does() {
        assert_eq!(
            body_of(b"HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n{\"a\":1}").unwrap(),
            b"{\"a\":1}"
        );
        assert_eq!(
            body_of(b"HTTP/1.1 403 Forbidden\r\n\r\nno")
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            body_of(b"HTTP/1.1 404 Not Found\r\n\r\n")
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        for bad in [
            &b"HTTP/1.1 500 Oops\r\n\r\n"[..],
            b"garbage",
            b"HTTP/2 200 OK\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\n",
        ] {
            assert_eq!(body_of(bad).unwrap_err().kind(), io::ErrorKind::InvalidData);
        }
    }

    #[cfg(unix)]
    #[test]
    fn only_a_plain_local_api_path_is_asked() {
        let socket = Path::new("/nonexistent");
        for bad in [
            "/other",
            "/localapi/v0/status HTTP/1.1",
            "/localapi/v0/status\r\nX: y",
        ] {
            assert_eq!(
                get_at(socket, bad, 0).unwrap_err().kind(),
                io::ErrorKind::InvalidInput,
                "{bad:?}"
            );
        }
    }
}
