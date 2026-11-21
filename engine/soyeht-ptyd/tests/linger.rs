//! A supervisor that ends a connection still reads what the peer sends for a moment, so the peer
//! is not reset on its unread input (on Windows a reset also discards what the supervisor wrote
//! and the peer had not read). The test is the same on every system: it sees the supervisor read.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use soyeht_platform::local::connect_proved;
use soyeht_protocol::frame::{Frame, read_json, write_json};
use soyeht_ptyd::wire::{Response, VERSION};
use std::io::Write;
use std::net::Shutdown;

mod support;
use support::{ASYNC_ASSERTION_TIMEOUT, Daemon};

/// A frame whose JSON is not a request: the supervisor ends the connection on it.
const UNDECODABLE: [u8; 7] = [1, 0, 0, 0, 2, b'{', b'}'];
/// What a peer sends after that: less than the supervisor reads before it gives up.
const AFTERWARDS: usize = 32 * 1024;

/// Waits for the supervisor to end the connection (its half-close reads as the end of the stream,
/// after any frames it sent), then writes `AFTERWARDS` bytes in pieces and ends the peer's input.
/// A supervisor that closed on unread input fails a write. Nothing here depends on how long the
/// supervisor takes: the writes follow its half-close at once, within its drain.
fn write_after_the_end(stream: &mut soyeht_platform::local::Stream) {
    stream
        .set_read_timeout(Some(ASYNC_ASSERTION_TIMEOUT))
        .unwrap();
    while let Some(frame) = Frame::read_from(&mut *stream).unwrap() {
        drop(frame);
    }
    for _ in 0..AFTERWARDS / 1024 {
        stream.write_all(&[0; 1024]).unwrap();
    }
    stream.shutdown(Shutdown::Write).unwrap();
}

#[test]
fn a_request_connection_that_sent_an_undecodable_frame_is_still_read_after_it_ended() {
    let (_daemon, socket) = Daemon::in_scratch_folder();
    let mut stream = connect_proved(&socket).unwrap();
    stream.write_all(&UNDECODABLE).unwrap();
    write_after_the_end(&mut stream);
}

#[test]
fn a_watch_connection_that_sent_an_undecodable_frame_gets_its_welcome_and_is_still_read() {
    let (_daemon, socket) = Daemon::in_scratch_folder();
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
    write_after_the_end(&mut stream);
}
