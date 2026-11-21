//! A supervisor that ends a connection still reads what the peer sends for a moment, so the peer
//! is not reset on its unread input (on Windows a reset also discards what the supervisor wrote
//! and the peer had not read). The test is the same on every system: it sees the supervisor read.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use soyeht_platform::local::connect_proved;
use soyeht_platform::testing::scratch_dir;
use soyeht_protocol::frame::{Frame, read_json, write_json};
use soyeht_ptyd::wire::{Response, VERSION};
use std::io::Write;
use std::net::Shutdown;
use std::time::Duration;

mod support;
use support::{ASYNC_ASSERTION_TIMEOUT, Daemon};

/// A frame whose JSON is not a request: the supervisor ends the connection on it.
const UNDECODABLE: [u8; 7] = [1, 0, 0, 0, 2, b'{', b'}'];
/// What a peer sends after that: more than a socket's buffer holds back for a closed peer, less
/// than the supervisor reads before it gives up.
const AFTERWARDS: usize = 32 * 1024;

fn daemon() -> (Daemon, std::path::PathBuf) {
    let directory = scratch_dir("ptyd");
    let daemon = Daemon {
        child: Daemon::launch(&directory),
        directory,
    };
    drop(daemon.connect(1));
    let socket = daemon.directory.join("ptyd.sock");
    (daemon, socket)
}

/// Writes `AFTERWARDS` bytes in pieces, after the supervisor has had the time to end the
/// connection, and ends the peer's input. A supervisor that closed on unread input fails a write.
fn write_afterwards(stream: &mut soyeht_platform::local::Stream) {
    std::thread::sleep(Duration::from_millis(300));
    for _ in 0..AFTERWARDS / 1024 {
        stream.write_all(&[0; 1024]).unwrap();
    }
    stream.shutdown(Shutdown::Write).unwrap();
}

#[test]
fn a_request_connection_that_sent_an_undecodable_frame_is_still_read_after_it_ended() {
    let (_daemon, socket) = daemon();
    let mut stream = connect_proved(&socket).unwrap();
    stream.write_all(&UNDECODABLE).unwrap();
    write_afterwards(&mut stream);
}

#[test]
fn a_watch_connection_that_sent_an_undecodable_frame_gets_its_welcome_and_is_still_read() {
    let (_daemon, socket) = daemon();
    let mut stream = connect_proved(&socket).unwrap();
    stream
        .set_read_timeout(Some(ASYNC_ASSERTION_TIMEOUT))
        .unwrap();
    write_json(
        &mut stream,
        &json!({"method":"hello","params":{"version":VERSION,"engine_generation":1,"role":"watch"}}),
    )
    .unwrap();
    assert!(matches!(
        read_json::<Response>(&mut stream).unwrap(),
        Some(Response::Welcome { .. })
    ));
    stream.write_all(&UNDECODABLE).unwrap();
    write_afterwards(&mut stream);
    // Nothing but whole frames comes back, then the end.
    while let Some(frame) = Frame::read_from(&mut stream).unwrap() {
        drop(frame);
    }
}
