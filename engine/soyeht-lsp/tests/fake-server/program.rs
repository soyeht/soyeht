#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use soyeht_lsp::{Decoder, encode};
use std::io::{self, Read, Write};

fn main() -> io::Result<()> {
    let mut decoder = Decoder::default();
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut bytes = [0_u8; 4096];
    loop {
        let count = input.read(&mut bytes)?;
        if count == 0 {
            return Ok(());
        }
        let messages = decoder.push(&bytes[..count]).map_err(io::Error::other)?;
        for message in messages {
            for response in respond(&message) {
                output.write_all(&encode(&response).map_err(io::Error::other)?)?;
                output.flush()?;
            }
        }
    }
}

fn respond(message: &Value) -> Vec<Value> {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Vec::new();
    };
    let id = message.get("id").cloned();
    match (method, id) {
        ("initialize", Some(id)) => vec![reply(
            id,
            json!({"capabilities":{
            "positionEncoding":"utf-16","textDocumentSync":1,"documentSymbolProvider":true,
            "definitionProvider":true,"referencesProvider":true,"documentFormattingProvider":true}}),
        )],
        ("textDocument/documentSymbol", Some(id)) => vec![reply(
            id,
            json!([{
            "name":"main","kind":12,"range":range(0,0,2,1),"selectionRange":range(0,3,0,7)}]),
        )],
        ("textDocument/definition", Some(id)) => vec![reply(
            id,
            json!({
            "uri":uri(message),"range":range(0,3,0,7)}),
        )],
        ("textDocument/references", Some(id)) => vec![reply(
            id,
            json!([{
            "uri":uri(message),"range":range(1,4,1,8)}, {
            "uri":"file:///outside/private.rs","range":range(0,0,0,1)}]),
        )],
        ("textDocument/formatting", Some(id)) => vec![reply(
            id,
            json!([{
            "range":range(0,0,0,0),"newText":"// formatted\n"}]),
        )],
        ("shutdown", Some(id)) => vec![reply(id, Value::Null)],
        ("textDocument/didOpen" | "textDocument/didChange", None) => vec![json!({
            "jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{
                "uri":uri(message),"diagnostics":[{
                    "range":range(1,4,1,8),"severity":2,
                    "message":"deterministic warning","source":"soyeht-fake-lsp"}]}})],
        _ => Vec::new(),
    }
}

fn reply(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn range(sl: u32, sc: u32, el: u32, ec: u32) -> Value {
    json!({"start":{"line":sl,"character":sc},"end":{"line":el,"character":ec}})
}

fn uri(message: &Value) -> Value {
    message
        .pointer("/params/textDocument/uri")
        .cloned()
        .unwrap_or_else(|| json!("file:///workspace/main.rs"))
}
