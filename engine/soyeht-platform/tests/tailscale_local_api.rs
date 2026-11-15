//! The client of the Tailscale daemon's local API against a daemon that answers as the test says:
//! a Unix socket in a private folder.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_platform::tailscale::{DEADLINE, MAX_BODY, get_at};
use std::{
    io::{self, Read as _, Write as _},
    os::unix::{fs::PermissionsExt as _, net::UnixListener},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

/// The account that runs the test, which owns the sockets it makes.
fn me() -> u32 {
    // SAFETY: geteuid takes no arguments and cannot fail.
    unsafe { libc::geteuid() }
}

/// A fresh socket path in a folder that only this account can use.
fn private_socket(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("sn-tailscale-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.join("tailscaled.sock");
    let _ = std::fs::remove_file(&socket);
    socket
}

/// A daemon that reads one request and answers `answer(request)`, then closes.
fn start_daemon(
    name: &str,
    answer: impl FnOnce(&str) -> Vec<u8> + Send + 'static,
) -> (PathBuf, thread::JoinHandle<()>) {
    let socket = private_socket(name);
    let listener = UnixListener::bind(&socket).unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut byte = [0u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            if stream.read(&mut byte).unwrap_or(0) == 0 {
                break;
            }
            request.push(byte[0]);
        }
        let reply = answer(&String::from_utf8_lossy(&request));
        // The client may have given up on a late answer.
        let _ = stream.write_all(&reply);
    });
    (socket, handle)
}

#[test]
fn a_request_names_the_local_api_host_and_the_body_comes_back() {
    let (socket, thread) = start_daemon("body", |request| {
        assert!(
            request.starts_with("GET /localapi/v0/whois?addr=100.64.0.9 HTTP/1.0\r\n"),
            "{request}"
        );
        assert!(
            request.contains("Host: local-tailscaled.sock\r\n"),
            "{request}"
        );
        b"HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n{\"Node\":{}}".to_vec()
    });
    let body = get_at(&socket, "/localapi/v0/whois?addr=100.64.0.9", me()).unwrap();
    assert_eq!(body, br#"{"Node":{}}"#);
    thread.join().unwrap();
}

#[test]
fn a_daemon_that_refuses_the_account_or_does_not_know_the_path_says_so() {
    for (status, kind) in [
        ("403 Forbidden", io::ErrorKind::PermissionDenied),
        ("404 Not Found", io::ErrorKind::NotFound),
        ("500 Internal Server Error", io::ErrorKind::InvalidData),
    ] {
        let (socket, thread) = start_daemon("status", move |_| {
            format!("HTTP/1.0 {status}\r\n\r\nno").into_bytes()
        });
        let error = get_at(&socket, "/localapi/v0/status", me()).unwrap_err();
        assert_eq!(error.kind(), kind, "{status}");
        thread.join().unwrap();
    }
}

#[test]
fn no_daemon_is_an_error_and_an_oversize_or_late_answer_is_too() {
    let nowhere = std::env::temp_dir().join("sn-tailscale-nowhere/tailscaled.sock");
    assert!(get_at(&nowhere, "/localapi/v0/status", me()).is_err());
    let (socket, thread) = start_daemon("big", |_| {
        let mut reply = b"HTTP/1.0 200 OK\r\n\r\n".to_vec();
        reply.extend(std::iter::repeat_n(b'x', MAX_BODY + 100_000));
        reply
    });
    let error = get_at(&socket, "/localapi/v0/status", me()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    thread.join().unwrap();
    // A daemon that accepts and says nothing is waited for no longer than the deadline.
    let (socket, thread) = start_daemon("late", |_| {
        thread::sleep(DEADLINE + Duration::from_millis(500));
        Vec::new()
    });
    let started = Instant::now();
    let error = get_at(&socket, "/localapi/v0/status", me()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(started.elapsed() < DEADLINE + Duration::from_secs(2));
    thread.join().unwrap();
}

#[test]
fn a_socket_or_folder_that_is_not_the_trusted_account_s_is_never_asked() {
    // The daemon would answer, but the caller trusts another account (root, in the product).
    let (socket, thread) = start_daemon("owner", |_| b"HTTP/1.0 200 OK\r\n\r\n{}".to_vec());
    let other = me().wrapping_add(1);
    let error = get_at(&socket, "/localapi/v0/status", other).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    // A folder that others may write could have the socket replaced under it.
    let folder = socket.parent().unwrap();
    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o777)).unwrap();
    let error = get_at(&socket, "/localapi/v0/status", me()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700)).unwrap();
    // A plain file in the socket's place is no daemon.
    let file = folder.join("plain");
    std::fs::write(&file, "x").unwrap();
    let error = get_at(&file, "/localapi/v0/status", me()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    // The daemon is still there for the account that is trusted.
    assert!(get_at(&socket, "/localapi/v0/status", me()).is_ok());
    thread.join().unwrap();
}

/// A daemon that never accepts: its queue holds one connection and nobody reads it.
#[cfg(target_os = "linux")]
#[test]
fn a_daemon_that_does_not_accept_is_a_timeout_and_not_a_hang() {
    use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
    let socket = private_socket("full");
    let unix_address = |path: &std::path::Path| {
        // SAFETY: an all-zero sockaddr_un is a valid value of the type.
        let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        address.sun_family = libc::AF_UNIX as libc::sa_family_t;
        for (slot, byte) in address
            .sun_path
            .iter_mut()
            .zip(path.as_os_str().as_encoded_bytes())
        {
            *slot = *byte as libc::c_char;
        }
        address
    };
    // SAFETY: plain system calls on descriptors this test owns; the addresses outlive the calls.
    let listener = unsafe {
        let raw = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0);
        assert!(raw >= 0);
        let listener = OwnedFd::from_raw_fd(raw);
        let address = unix_address(&socket);
        let size = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
        assert_eq!(
            libc::bind(
                listener.as_raw_fd(),
                std::ptr::from_ref(&address).cast(),
                size
            ),
            0
        );
        assert_eq!(libc::listen(listener.as_raw_fd(), 0), 0);
        listener
    };
    // Fill the queue with connections that are never accepted.
    let mut filling = Vec::new();
    for _ in 0..8 {
        // SAFETY: as above.
        unsafe {
            let raw = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_NONBLOCK, 0);
            let client = OwnedFd::from_raw_fd(raw);
            let address = unix_address(&socket);
            let size = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
            libc::connect(
                client.as_raw_fd(),
                std::ptr::from_ref(&address).cast(),
                size,
            );
            filling.push(client);
        }
    }
    let started = Instant::now();
    let error = get_at(&socket, "/localapi/v0/status", me()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut, "{error}");
    assert!(started.elapsed() < DEADLINE + Duration::from_secs(2));
    drop((listener, filling));
}
