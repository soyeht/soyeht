#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::Value;
use soyeht_lsp::{Client, Decoder, Incoming, RequestKind, encode};
use std::io::{Read, Write};
use std::process::{Command, Stdio};

#[test]
fn deterministic_server_handles_language_operations_and_diagnostics() {
    // The fake writes what it was started with into its working directory.
    let folder = std::env::temp_dir().join(format!("sn-fake-lsp-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_soyeht-fake-lsp"))
        .current_dir(&folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let mut client = Client::default();
    let initialize = client.initialize("file:///workspace", None).unwrap();
    input.write_all(&encode(&initialize).unwrap()).unwrap();
    let response = read_one(&mut output);
    assert!(
        matches!(client.accept(response).unwrap(), Incoming::Response { call, .. } if call.kind == RequestKind::Initialize)
    );
    let opened = soyeht_lsp::did_open("file:///workspace/main.rs", "rust", 1, "fn main() {}");
    input.write_all(&encode(&opened).unwrap()).unwrap();
    let event = client.accept(read_one(&mut output)).unwrap();
    assert!(matches!(event, Incoming::Diagnostics { diagnostics, .. } if diagnostics.len() == 1));
    child.kill().unwrap();
    child.wait().unwrap();
    std::fs::remove_dir_all(folder).unwrap();
}

fn read_one(reader: &mut impl Read) -> Value {
    let mut decoder = Decoder::default();
    let mut bytes = [0_u8; 128];
    loop {
        let count = reader.read(&mut bytes).unwrap();
        let values = decoder.push(&bytes[..count]).unwrap();
        if let Some(value) = values.into_iter().next() {
            return value;
        }
    }
}
