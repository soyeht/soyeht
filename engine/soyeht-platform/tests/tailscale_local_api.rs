//! The client of the Tailscale daemon's local API against a daemon that answers as the test says:
//! a Unix socket in a private folder.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_platform::tailscale::{DEADLINE, MAX_BODY, get_at};
use std::{
    io::{self, Read as _, Write as _},
    os::unix::net::UnixListener,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

/// A daemon that reads one request and answers `answer(request)`, then closes.
fn start_daemon(
    name: &str,
    answer: impl FnOnce(&str) -> Vec<u8> + Send + 'static,
) -> (PathBuf, thread::JoinHandle<()>) {
    let directory =
        std::env::temp_dir().join(format!("sn-tailscale-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let socket = directory.join("tailscaled.sock");
    let _ = std::fs::remove_file(&socket);
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
    let body = get_at(&socket, "/localapi/v0/whois?addr=100.64.0.9").unwrap();
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
        let error = get_at(&socket, "/localapi/v0/status").unwrap_err();
        assert_eq!(error.kind(), kind, "{status}");
        thread.join().unwrap();
    }
}

#[test]
fn no_daemon_is_an_error_and_an_oversize_or_late_answer_is_too() {
    let nowhere = std::env::temp_dir().join("sn-tailscale-nowhere/tailscaled.sock");
    assert!(get_at(&nowhere, "/localapi/v0/status").is_err());
    let (socket, thread) = start_daemon("big", |_| {
        let mut reply = b"HTTP/1.0 200 OK\r\n\r\n".to_vec();
        reply.extend(std::iter::repeat_n(b'x', MAX_BODY + 100_000));
        reply
    });
    let error = get_at(&socket, "/localapi/v0/status").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    thread.join().unwrap();
    // A daemon that accepts and says nothing is waited for no longer than the deadline.
    let (socket, thread) = start_daemon("late", |_| {
        thread::sleep(DEADLINE + Duration::from_millis(500));
        Vec::new()
    });
    let started = Instant::now();
    let error = get_at(&socket, "/localapi/v0/status").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(started.elapsed() < DEADLINE + Duration::from_secs(2));
    thread.join().unwrap();
}
