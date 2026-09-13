use crate::model::{diagnostic, edit, location, symbol};
use crate::{Diagnostic, Error, Location, Result, Symbol, TextEdit};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const PENDING_LIMIT: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestKind {
    Initialize,
    Symbols,
    Definition,
    References,
    Formatting,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    pub kind: RequestKind,
    pub context: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Response {
        call: Call,
        result: Value,
    },
    Diagnostics {
        uri: String,
        diagnostics: Vec<Diagnostic>,
    },
    Notification {
        method: String,
        params: Value,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Client {
    next_id: u64,
    pending: BTreeMap<u64, Call>,
}

impl Client {
    pub fn request(&mut self, kind: RequestKind, params: Value, context: &str) -> Result<Value> {
        if self.pending.len() >= PENDING_LIMIT {
            return Err(Error::RequestLimit);
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(Error::RequestIdExhausted)?;
        self.pending.insert(
            self.next_id,
            Call {
                kind,
                context: context.to_owned(),
            },
        );
        Ok(json!({"jsonrpc":"2.0","id":self.next_id,"method":method(kind),"params":params}))
    }

    pub fn initialize(&mut self, root_uri: &str) -> Result<Value> {
        self.request(
            RequestKind::Initialize,
            json!({
                "processId":null,
                "rootUri":root_uri,
                "capabilities":{
                    "general":{"positionEncodings":["utf-16"]},
                    "textDocument":{
                        "publishDiagnostics":{},"documentSymbol":{},"definition":{},
                        "references":{},"formatting":{}
                    }
                },
                "clientInfo":{"name":"Soyeht","version":env!("CARGO_PKG_VERSION")}
            }),
            "initialize",
        )
    }

    pub fn symbols(&mut self, uri: &str, context: &str) -> Result<Value> {
        self.request(RequestKind::Symbols, document(uri), context)
    }

    pub fn definition(
        &mut self,
        uri: &str,
        position: PositionValue,
        context: &str,
    ) -> Result<Value> {
        self.request(RequestKind::Definition, position.params(uri), context)
    }

    pub fn references(
        &mut self,
        uri: &str,
        position: PositionValue,
        context: &str,
    ) -> Result<Value> {
        let mut params = position.params(uri);
        params["context"] = json!({"includeDeclaration":true});
        self.request(RequestKind::References, params, context)
    }

    pub fn formatting(&mut self, uri: &str, context: &str) -> Result<Value> {
        self.request(
            RequestKind::Formatting,
            json!({"textDocument":{"uri":uri},"options":{"tabSize":4,"insertSpaces":true}}),
            context,
        )
    }

    pub fn accept(&mut self, value: Value) -> Result<Incoming> {
        if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(Error::InvalidMessage);
        }
        if let Some(method) = value.get("method").and_then(Value::as_str) {
            let params = value.get("params").cloned().unwrap_or_else(|| json!({}));
            if method == "textDocument/publishDiagnostics" {
                return diagnostics(params);
            }
            return Ok(Incoming::Notification {
                method: method.to_owned(),
                params,
            });
        }
        let id = value
            .get("id")
            .and_then(Value::as_u64)
            .ok_or(Error::InvalidResponseId)?;
        let call = self.pending.remove(&id).ok_or(Error::UnmatchedResponse)?;
        if let Some(error) = value.get("error") {
            return Err(Error::Remote {
                code: error
                    .get("code")
                    .and_then(Value::as_i64)
                    .ok_or(Error::InvalidMessage)?,
                message: error
                    .get("message")
                    .and_then(Value::as_str)
                    .ok_or(Error::InvalidMessage)?
                    .to_owned(),
            });
        }
        let result = value.get("result").cloned().ok_or(Error::InvalidMessage)?;
        Ok(Incoming::Response { call, result })
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PositionValue {
    pub line: u32,
    pub character: u32,
}

impl PositionValue {
    fn params(self, uri: &str) -> Value {
        json!({"textDocument":{"uri":uri},"position":{"line":self.line,"character":self.character}})
    }
}

pub fn initialized() -> Value {
    json!({"jsonrpc":"2.0","method":"initialized","params":{}})
}

pub fn did_open(uri: &str, language_id: &str, version: i64, text: &str) -> Value {
    json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{
        "uri":uri,"languageId":language_id,"version":version,"text":text}}})
}

pub fn did_change(uri: &str, version: i64, text: &str) -> Value {
    json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{
        "uri":uri,"version":version},"contentChanges":[{"text":text}]}})
}

pub fn symbols(value: &Value) -> Result<Vec<Symbol>> {
    array_or_null(value)?.iter().map(symbol).collect()
}

pub fn locations(value: &Value) -> Result<Vec<Location>> {
    if value.is_object() {
        return Ok(vec![location(value)?]);
    }
    array_or_null(value)?.iter().map(location).collect()
}

pub fn edits(value: &Value) -> Result<Vec<TextEdit>> {
    array_or_null(value)?.iter().map(edit).collect()
}

fn diagnostics(params: Value) -> Result<Incoming> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .filter(|value| value.len() <= 4096)
        .ok_or(Error::InvalidPayload("diagnostic uri"))?
        .to_owned();
    let values = params
        .get("diagnostics")
        .and_then(Value::as_array)
        .ok_or(Error::InvalidPayload("diagnostics"))?;
    if values.len() > 500 {
        return Err(Error::InvalidPayload("diagnostics"));
    }
    let diagnostics = values.iter().map(diagnostic).collect::<Result<_>>()?;
    Ok(Incoming::Diagnostics { uri, diagnostics })
}

fn array_or_null(value: &Value) -> Result<&[Value]> {
    if value.is_null() {
        return Ok(&[]);
    }
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or(Error::InvalidPayload("result"))
}

fn document(uri: &str) -> Value {
    json!({"textDocument":{"uri":uri}})
}

fn method(kind: RequestKind) -> &'static str {
    match kind {
        RequestKind::Initialize => "initialize",
        RequestKind::Symbols => "textDocument/documentSymbol",
        RequestKind::Definition => "textDocument/definition",
        RequestKind::References => "textDocument/references",
        RequestKind::Formatting => "textDocument/formatting",
        RequestKind::Shutdown => "shutdown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_state_preserves_request_correlation_and_utf16_capability() {
        let mut client = Client::default();
        let initialize = client.initialize("file:///workspace").unwrap();
        assert_eq!(
            initialize["params"]["capabilities"]["general"]["positionEncodings"][0],
            "utf-16"
        );
        let bytes = serde_json::to_vec(&client).unwrap();
        let mut restored: Client = serde_json::from_slice(&bytes).unwrap();
        let incoming = restored
            .accept(json!({"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}))
            .unwrap();
        assert!(matches!(
            incoming,
            Incoming::Response {
                call: Call {
                    kind: RequestKind::Initialize,
                    ..
                },
                ..
            }
        ));
        assert_eq!(restored.pending(), 0);
    }

    #[test]
    fn mappings_cover_diagnostics_symbols_locations_and_edits() {
        let range = json!({"start":{"line":1,"character":2},"end":{"line":1,"character":5}});
        let mut client = Client::default();
        let event = client.accept(json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{
            "uri":"file:///workspace/main.rs","diagnostics":[{"range":range,"severity":2,"message":"warning","source":"fake"}]}})).unwrap();
        assert!(
            matches!(event, Incoming::Diagnostics { diagnostics, .. } if diagnostics[0].severity == soyeht_protocol::DiagnosticSeverity::Warning)
        );
        assert_eq!(
            symbols(&json!([{"name":"main","kind":12,"range":range,"selectionRange":range}]))
                .unwrap()[0]
                .name,
            "main"
        );
        assert_eq!(
            locations(&json!({"uri":"file:///workspace/main.rs","range":range}))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            edits(&json!([{"range":range,"newText":"formatted"}])).unwrap()[0].text,
            "formatted"
        );
    }
}
