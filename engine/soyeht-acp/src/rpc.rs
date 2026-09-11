use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use soyeht_protocol::limits;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}
impl std::error::Error for Error {}
impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self(error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    Response {
        id: Value,
        result: Result<Value, RpcError>,
    },
}

fn valid_id(id: &Value) -> bool {
    id.is_null() || id.is_string() || id.is_i64()
}

impl Message {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > limits::ACP_MESSAGE_BYTES {
            return Err(Error("ACP line exceeds 1 MiB".into()));
        }
        let value: Value = serde_json::from_slice(bytes)?;
        if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(Error("JSON-RPC 2.0 envelope required".into()));
        }
        if let Some(method) = value.get("method") {
            let method = method
                .as_str()
                .filter(|method| !method.is_empty() && method.len() <= 256)
                .ok_or_else(|| Error("invalid ACP method".into()))?
                .to_owned();
            if value.get("result").is_some() || value.get("error").is_some() {
                return Err(Error("request contains a response".into()));
            }
            let params = value.get("params").cloned().unwrap_or_else(|| json!({}));
            if !params.is_object() {
                return Err(Error("ACP parameters must be an object".into()));
            }
            return match value.get("id") {
                Some(id) if valid_id(id) => Ok(Self::Request {
                    id: id.clone(),
                    method,
                    params,
                }),
                Some(_) => Err(Error("invalid ACP request ID".into())),
                None => Ok(Self::Notification { method, params }),
            };
        }
        let id = value
            .get("id")
            .filter(|id| id.is_null() || valid_id(id))
            .ok_or_else(|| Error("invalid ACP response ID".into()))?
            .clone();
        let result = match (value.get("result"), value.get("error")) {
            (Some(result), None) => Ok(result.clone()),
            (None, Some(error)) => Err(serde_json::from_value(error.clone())?),
            _ => return Err(Error("response needs exactly one result or error".into())),
        };
        Ok(Self::Response { id, result })
    }

    pub fn value(&self) -> Value {
        match self {
            Self::Request { id, method, params } => {
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
            }
            Self::Notification { method, params } => {
                json!({"jsonrpc":"2.0","method":method,"params":params})
            }
            Self::Response {
                id,
                result: Ok(result),
            } => json!({"jsonrpc":"2.0","id":id,"result":result}),
            Self::Response {
                id,
                result: Err(error),
            } => json!({"jsonrpc":"2.0","id":id,"error":error}),
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        let mut bytes = serde_json::to_vec(&self.value())?;
        if bytes.len() > limits::ACP_MESSAGE_BYTES {
            return Err(Error("ACP line exceeds 1 MiB".into()));
        }
        bytes.push(b'\n');
        Ok(bytes)
    }
}

#[derive(Debug)]
pub struct Record {
    pub from_seq: u64,
    pub to_seq: u64,
    pub message: Message,
}

/// The engine acknowledges only complete records. Partial bytes can be reread
/// from ptyd after a crash instead of being reinterpreted at a new boundary.
pub struct Decoder {
    bytes: Vec<u8>,
    start: u64,
    next: u64,
}

impl Decoder {
    pub fn new(offset: u64) -> Self {
        Self {
            bytes: Vec::new(),
            start: offset,
            next: offset,
        }
    }
    pub fn next_offset(&self) -> u64 {
        self.next
    }
    pub fn push(&mut self, offset: u64, bytes: &[u8]) -> Result<Vec<Record>, Error> {
        if offset != self.next {
            return Err(Error("ACP stream has a gap or duplicate bytes".into()));
        }
        let mut records = Vec::new();
        for &byte in bytes {
            self.next = self
                .next
                .checked_add(1)
                .ok_or_else(|| Error("ACP offset overflow".into()))?;
            if byte == b'\n' {
                let message = Message::parse(&self.bytes)?;
                records.push(Record {
                    from_seq: self.start,
                    to_seq: self.next,
                    message,
                });
                self.bytes.clear();
                self.start = self.next;
            } else {
                if self.bytes.len() == limits::ACP_MESSAGE_BYTES {
                    return Err(Error("ACP line exceeds 1 MiB".into()));
                }
                self.bytes.push(byte);
            }
        }
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragmented_utf8_and_multiple_lines_keep_exact_record_offsets() {
        let message = Message::Notification {
            method: "session/update".into(),
            params: json!({"text":"Hello 🌍"}),
        };
        let bytes = message.encode().unwrap();
        let mut decoder = Decoder::new(31);
        let mut records = Vec::new();
        for byte in &bytes {
            records.extend(decoder.push(decoder.next_offset(), &[*byte]).unwrap());
        }
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].from_seq, 31);
        assert_eq!(records[0].to_seq, 31 + bytes.len() as u64);
        assert_eq!(records[0].message, message);
        assert!(decoder.push(31, &bytes).is_err());
    }

    #[test]
    fn rejects_ambiguous_envelopes_invalid_ids_and_unbounded_lines() {
        for value in [
            json!({"id":1,"result":{}}),
            json!({"jsonrpc":"2.0","id":1.5,"result":{}}),
            json!({"jsonrpc":"2.0","id":1,"result":{},"error":{"code":-1,"message":"x"}}),
            json!({"jsonrpc":"2.0","id":1,"method":"x","result":{}}),
            json!({"jsonrpc":"2.0","method":"x","params":[]}),
        ] {
            assert!(Message::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        }
        let mut decoder = Decoder::new(0);
        assert!(
            decoder
                .push(0, &vec![b'x'; limits::ACP_MESSAGE_BYTES + 1])
                .is_err()
        );
    }
}
