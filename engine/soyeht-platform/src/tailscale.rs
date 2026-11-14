//! The local API of the Tailscale daemon, which tells who a peer address is (ADR 0164).
//!
//! The daemon listens on a Unix socket that any local account may read from on Linux. A request
//! is plain HTTP over it. This module only carries a `GET` and returns the body: what the answer
//! means is the caller's. It never blocks longer than [`DEADLINE`], and an answer that does not
//! fit [`MAX_BODY`] is an error. Only Linux has the socket at a known place: elsewhere
//! [`get`] answers [`io::ErrorKind::Unsupported`].

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
/// error (`NotFound` or `ConnectionRefused` when no daemon runs, `PermissionDenied` when the
/// account may not use it), `InvalidInput` for a `target` that is not a plain local API path,
/// and `InvalidData` for an answer that is not a `200` with a body that fits.
#[cfg(target_os = "linux")]
pub fn get(target: &str) -> io::Result<Vec<u8>> {
    get_at(Path::new(LINUX_SOCKET), target)
}

/// See the Linux form.
#[cfg(not(target_os = "linux"))]
pub fn get(_target: &str) -> io::Result<Vec<u8>> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// [`get`] on the daemon socket at `socket`.
#[cfg(unix)]
pub fn get_at(socket: &Path, target: &str) -> io::Result<Vec<u8>> {
    if !target.starts_with("/localapi/v0/") || target.bytes().any(|byte| byte <= b' ') {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    unix::get(socket, target)
}

/// See the Unix form.
#[cfg(not(unix))]
pub fn get_at(_socket: &Path, _target: &str) -> io::Result<Vec<u8>> {
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
        os::unix::net::UnixStream,
        path::Path,
        time::Instant,
    };

    /// What the daemon expects in `Host` on its socket.
    const HOST: &str = "local-tailscaled.sock";

    pub(super) fn get(socket: &Path, target: &str) -> io::Result<Vec<u8>> {
        let started = Instant::now();
        let mut stream = UnixStream::connect(socket)?;
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
                get_at(socket, bad).unwrap_err().kind(),
                io::ErrorKind::InvalidInput,
                "{bad:?}"
            );
        }
    }
}
