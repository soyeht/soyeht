//! Request recovery is independent of acknowledgement of the raw byte stream.
use crate::wire::{PendingRequest, PendingRequestChunk};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;
use soyeht_protocol::limits;
use std::collections::BTreeMap;

const MAX_PENDING: usize = 128;
const MAX_PENDING_BYTES: usize = 8 * 1024 * 1024;

#[derive(Default)]
struct Lines {
    bytes: Vec<u8>,
    next: u64,
    start: u64,
    oversized: bool,
}

impl Lines {
    fn feed(&mut self, bytes: &[u8]) -> (Vec<(u64, u64, Vec<u8>)>, bool) {
        let mut lines = Vec::new();
        let mut fault = false;
        for &byte in bytes {
            self.next += 1;
            if byte == b'\n' {
                if !self.oversized && !self.bytes.is_empty() {
                    lines.push((self.start, self.next, std::mem::take(&mut self.bytes)));
                }
                self.oversized = false;
                self.start = self.next;
            } else if !self.oversized {
                if self.bytes.len() == limits::ACP_MESSAGE_BYTES {
                    self.bytes.clear();
                    self.oversized = true;
                    fault = true;
                } else {
                    self.bytes.push(byte);
                }
            }
        }
        (lines, fault)
    }
}

struct Stored {
    metadata: PendingRequest,
    native_id: String,
    bytes: Vec<u8>,
}

#[derive(Default)]
pub struct Pending {
    output: Lines,
    input: Lines,
    requests: BTreeMap<u64, Stored>,
    bytes: usize,
    fault: bool,
}

fn id(value: &Value) -> Option<String> {
    let value = value.get("id")?;
    if !(value.is_null() || value.is_string() || value.is_i64() || value.is_u64()) {
        return None;
    }
    Some(value.to_string())
}

impl Pending {
    pub fn output(&mut self, bytes: &[u8]) {
        let (lines, fault) = self.output.feed(bytes);
        self.fault |= fault;
        for (start, end, bytes) in lines {
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                self.fault = true;
                continue;
            };
            if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
                self.fault = true;
                continue;
            }
            let (Some(native_id), Some(method)) =
                (id(&value), value.get("method").and_then(Value::as_str))
            else {
                continue;
            };
            if method.len() > 256
                || self.requests.len() == MAX_PENDING
                || self.bytes + bytes.len() > MAX_PENDING_BYTES
                || self
                    .requests
                    .values()
                    .any(|request| request.native_id == native_id)
            {
                self.fault = true;
                continue;
            }
            let metadata = PendingRequest {
                request_key: start,
                from_seq: start,
                to_seq: end,
                bytes: bytes.len(),
                method: method.into(),
            };
            self.bytes += bytes.len();
            self.requests.insert(
                start,
                Stored {
                    metadata,
                    native_id,
                    bytes,
                },
            );
        }
    }

    pub fn input(&mut self, bytes: &[u8]) {
        let (lines, fault) = self.input.feed(bytes);
        self.fault |= fault;
        for (_, _, bytes) in lines {
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                self.fault = true;
                continue;
            };
            if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
                self.fault = true;
                continue;
            }
            if value.get("method").is_some()
                || (value.get("result").is_some() == value.get("error").is_some())
            {
                continue;
            }
            let Some(native_id) = id(&value) else {
                continue;
            };
            let key = self
                .requests
                .iter()
                .find(|(_, request)| request.native_id == native_id)
                .map(|(key, _)| *key);
            if let Some(key) = key {
                let removed = self.requests.remove(&key).expect("existing request");
                self.bytes -= removed.bytes.len();
            }
        }
    }

    pub fn list(&self) -> (Vec<PendingRequest>, bool) {
        (
            self.requests
                .values()
                .map(|request| request.metadata.clone())
                .collect(),
            self.fault,
        )
    }

    pub fn chunk(&self, key: u64, offset: usize) -> Result<PendingRequestChunk, &'static str> {
        let request = self.requests.get(&key).ok_or("not_found")?;
        if offset > request.bytes.len() {
            return Err("invalid_offset");
        }
        let end = request
            .bytes
            .len()
            .min(offset.saturating_add(limits::TERMINAL_PAYLOAD_BYTES));
        Ok(PendingRequestChunk {
            request_key: key,
            offset,
            next_offset: end,
            total_bytes: request.bytes.len(),
            bytes: STANDARD.encode(&request.bytes[offset..end]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragmented_requests_survive_until_the_entire_response_was_written() {
        let mut pending = Pending::default();
        let native =
            br#"{"jsonrpc":"2.0","id":"7","method":"session/request_permission","params":{}}"#;
        pending.output(&native[..17]);
        assert!(pending.list().0.is_empty());
        pending.output(&native[17..]);
        pending.output(b"\n");
        let (requests, fault) = pending.list();
        assert!(!fault);
        assert_eq!(requests[0].from_seq, 0);
        assert_eq!(requests[0].to_seq, native.len() as u64 + 1);
        assert_eq!(
            STANDARD.decode(pending.chunk(0, 0).unwrap().bytes).unwrap(),
            native
        );
        // Numeric and string identifiers are different; outgoing requests are
        // also different from responses, even when they reuse a native ID.
        pending.input(b"{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{}}\n");
        pending.input(b"{\"jsonrpc\":\"2.0\",\"id\":\"7\",\"method\":\"session/new\"}\n");
        let response = b"{\"jsonrpc\":\"2.0\",\"id\":\"7\",\"result\":{}}\n";
        pending.input(&response[..response.len() - 1]);
        assert_eq!(pending.list().0.len(), 1);
        pending.input(b"\n");
        assert!(pending.list().0.is_empty());
        assert_eq!(pending.bytes, 0);
    }

    #[test]
    fn oversized_and_duplicate_requests_report_a_sticky_fault_without_growing() {
        let mut pending = Pending::default();
        pending.output(&vec![b'x'; limits::ACP_MESSAGE_BYTES + 1]);
        assert!(pending.output.bytes.is_empty());
        pending.output(b"\n");
        let request = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"test\"}\n";
        pending.output(request);
        pending.output(request);
        assert_eq!(pending.list().0.len(), 1);
        assert!(pending.list().1);
        assert_eq!(
            pending
                .chunk(limits::ACP_MESSAGE_BYTES as u64 + 2, 0)
                .unwrap()
                .total_bytes,
            request.len() - 1
        );
    }

    #[test]
    fn large_native_requests_are_paginated_and_the_pending_budget_is_enforced() {
        let mut pending = Pending::default();
        for id in 0..10 {
            let request = format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"test\",\"params\":\"{}\"}}\n",
                "x".repeat(1000000)
            );
            pending.output(request.as_bytes());
        }
        assert_eq!(pending.list().0.len(), 8);
        assert!(pending.list().1);
        assert!(pending.bytes <= MAX_PENDING_BYTES);
        let chunk = pending.chunk(0, 0).unwrap();
        assert_eq!(chunk.next_offset, limits::TERMINAL_PAYLOAD_BYTES);
        assert!(serde_json::to_vec(&chunk).unwrap().len() < limits::JSON_BYTES);
    }
    #[test]
    fn null_and_empty_request_ids_are_distinct_from_notifications() {
        let mut pending = Pending::default();
        for id in [
            serde_json::Value::Null,
            serde_json::json!(""),
            serde_json::json!("null"),
        ] {
            let line = format!(
                "{}\n",
                serde_json::json!({"jsonrpc":"2.0","id":id,"method":"session/request_permission","params":{}})
            );
            pending.output(line.as_bytes());
        }
        pending.output(b"{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{}}\n");
        assert_eq!(pending.list().0.len(), 3);
        pending.input(b"{\"jsonrpc\":\"2.0\",\"id\":null,\"result\":{}}\n");
        assert_eq!(pending.list().0.len(), 2);
        pending.input(b"{\"jsonrpc\":\"2.0\",\"id\":\"\",\"result\":{}}\n");
        assert_eq!(pending.list().0.len(), 1);
        assert!(!pending.list().1);
    }
}
