//! Fuzz targets for the supervisor wire: the request decoder, and a real supervisor that is sent
//! framed garbage before and after its `hello`. It must answer only with well-formed responses,
//! refuse a request before the `hello`, and still serve a good client afterwards.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use soyeht_platform::local::connect;
use soyeht_protocol::frame::{Frame, read_json, write_json};
use soyeht_ptyd::wire::{Request, Response, VERSION};
use std::io::{Read, Write};
use std::net::Shutdown;
use std::time::Duration;

#[path = "../../soyeht-protocol/tests/fuzz_support/mod.rs"]
mod fuzz_support;
use fuzz_support::{Corpus, fuzz_bytes, fuzz_json};

mod support;
use support::{ASYNC_ASSERTION_TIMEOUT, Daemon};

const CASES: u64 = 4_000;
/// Connections to a real supervisor per run: each is a socket, a thread and a round trip.
const WIRE_CASES: u64 = 120;

fn request_values() -> Vec<Value> {
    let id = "00000000-0000-4000-8000-000000000000";
    vec![
        json!({"method":"hello","params":{"version":VERSION,"engine_generation":1}}),
        json!({"method":"list"}),
        json!({"method":"close","params":{"instance_id":id}}),
        json!({"method":"kill","params":{"instance_id":id}}),
        json!({"method":"input","params":{"instance_id":id,"bytes":"AAAA","operation_key":"k","fingerprint":"f"}}),
        json!({"method":"confirm_input","params":{"operation_key":"k"}}),
        json!({"method":"pending_inputs"}),
        json!({"method":"pending_requests","params":{"instance_id":id}}),
        json!({"method":"pending_request_chunk","params":{"instance_id":id,"request_key":1,"offset":0}}),
        json!({"method":"resize","params":{"instance_id":id,"cols":80,"rows":24}}),
        json!({"method":"drain","params":{"instance_id":id,"from_seq":0}}),
        json!({"method":"ack","params":{"instance_id":id,"seq":0}}),
        json!({"method":"spawn","params":{"instance_key":"k","generation":1,"kind":"pty",
            "cwd":"/","argv":["/bin/sh"],"env":{},"cols":80,"rows":24}}),
    ]
}

#[test]
fn requests_from_any_value_decode_or_fail_without_a_panic() {
    fuzz_json("ptyd-wire-request", CASES, &request_values(), |value| {
        let _ = serde_json::from_value::<Request>(value.clone());
        let _ = serde_json::from_value::<Response>(value.clone());
    });
}

fn framed(value: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_json(&mut bytes, value).unwrap();
    bytes
}

/// Whether any frame of `bytes` decodes as a request that starts a process: the test sends
/// nothing that would.
fn would_spawn(bytes: &[u8]) -> bool {
    let mut reader = bytes;
    while let Ok(Some(frame)) = Frame::read_from(&mut reader) {
        if let Frame::Json(body) = frame
            && matches!(
                serde_json::from_slice::<Request>(&body),
                Ok(Request::Spawn(_))
            )
        {
            return true;
        }
    }
    false
}

#[test]
fn a_supervisor_refuses_a_request_before_hello_and_survives_framed_garbage() {
    let directory = std::env::temp_dir().join(format!("soyeht-fuzz-ptyd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    let mut daemon = Daemon {
        child: Daemon::launch(&directory),
        directory,
    };
    drop(daemon.connect(1));
    let socket = daemon.directory.join("ptyd.sock");

    // Any request before the hello is refused with the one error, and nothing is executed.
    for value in request_values() {
        let mut stream = connect(&socket).unwrap();
        stream
            .set_read_timeout(Some(ASYNC_ASSERTION_TIMEOUT))
            .unwrap();
        write_json(&mut stream, &value).unwrap();
        match read_json::<Response>(&mut stream).unwrap() {
            Some(Response::Error { kind, .. }) if value["method"] == "hello" => {
                assert_ne!(kind, "unauthorized")
            }
            Some(Response::Error { kind, .. }) => assert_eq!(kind, "unauthorized", "{value}"),
            Some(Response::Welcome { .. }) if value["method"] == "hello" => {}
            other => panic!("{value}: {other:?}"),
        }
    }

    let hello =
        framed(&json!({"method":"hello","params":{"version":VERSION,"engine_generation":1}}));
    let mut seeds: Vec<Vec<u8>> = request_values()
        .iter()
        .filter(|value| value["method"] != "spawn")
        .map(|value| {
            let mut bytes = hello.clone();
            bytes.extend(framed(value));
            bytes
        })
        .collect();
    seeds.push(hello.clone());
    seeds.push(vec![1, 0xff, 0xff, 0xff, 0xff]);
    seeds.push(vec![1, 0, 0, 0, 2, b'{', b'}']);
    let seeds: Vec<&[u8]> = seeds.iter().map(Vec::as_slice).collect();
    let corpus = Corpus {
        seeds: &seeds,
        tokens: &[
            b"\"method\"",
            b"\"params\"",
            b"{",
            b"}",
            b"\"",
            b"null",
            b"[",
            b"\xff",
        ],
        max_len: 400,
    };
    fuzz_bytes("ptyd-wire-connection", WIRE_CASES, &corpus, |input| {
        if would_spawn(input) {
            return;
        }
        let mut stream = connect(&socket).unwrap();
        stream
            .set_read_timeout(Some(ASYNC_ASSERTION_TIMEOUT))
            .unwrap();
        // The peer may close before all of the input is written; that is the supervisor ending
        // the connection, not a failure of the test.
        let _ = stream.write_all(input);
        let _ = stream.shutdown(Shutdown::Write);
        let mut answer = Vec::new();
        let _ = stream.read_to_end(&mut answer);
        // Everything the supervisor sent back is a whole, well-formed response.
        let mut reader = answer.as_slice();
        while let Some(response) = read_json::<Response>(&mut reader)
            .unwrap_or_else(|error| panic!("a malformed answer: {error:?}"))
        {
            drop(response);
        }
    });

    // The supervisor still serves a good client, and has not died.
    assert!(
        daemon.child.try_wait().unwrap().is_none(),
        "the supervisor ended"
    );
    let deadline = std::time::Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        match soyeht_ptyd::client::Client::connect(&socket, u64::MAX) {
            Ok(_) => break,
            Err(error) => {
                assert!(std::time::Instant::now() < deadline, "{error}");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}
