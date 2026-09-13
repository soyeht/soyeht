use crate::{Error, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use soyeht_protocol::limits;

const HEADER_BYTES: usize = 8192;
const SEPARATOR: &[u8] = b"\r\n\r\n";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Decoder {
    #[serde(with = "base64_bytes")]
    bytes: Vec<u8>,
    body_len: Option<usize>,
}

mod base64_bytes {
    use super::*;
    use serde::{Deserializer, Serializer, de::Error as _};

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> std::result::Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        STANDARD.decode(encoded).map_err(D::Error::custom)
    }
}

impl Decoder {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Value>> {
        let capacity = limits::JSON_BYTES.saturating_add(HEADER_BYTES);
        if self.bytes.len().saturating_add(chunk.len()) > capacity {
            return Err(Error::MessageTooLarge);
        }
        self.bytes.extend_from_slice(chunk);
        let mut values = Vec::new();
        loop {
            if self.body_len.is_none() {
                let Some(end) = find(&self.bytes, SEPARATOR) else {
                    if self.bytes.len() > HEADER_BYTES {
                        return Err(Error::HeaderTooLarge);
                    }
                    break;
                };
                if end > HEADER_BYTES {
                    return Err(Error::HeaderTooLarge);
                }
                self.body_len = Some(content_length(&self.bytes[..end])?);
                self.bytes.drain(..end + SEPARATOR.len());
            }
            let Some(length) = self.body_len else { break };
            if self.bytes.len() < length {
                break;
            }
            let body: Vec<u8> = self.bytes.drain(..length).collect();
            self.body_len = None;
            values.push(serde_json::from_slice(&body)?);
        }
        Ok(values)
    }
}

pub fn encode(value: &Value) -> Result<Vec<u8>> {
    let body = serde_json::to_vec(value)?;
    if body.len() > limits::JSON_BYTES {
        return Err(Error::MessageTooLarge);
    }
    let mut message = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    message.extend(body);
    Ok(message)
}

fn content_length(header: &[u8]) -> Result<usize> {
    let header = std::str::from_utf8(header).map_err(|_| Error::InvalidHeader)?;
    let mut result = None;
    for line in header.split("\r\n") {
        let Some((name, value)) = line.split_once(':') else {
            return Err(Error::InvalidHeader);
        };
        if name.eq_ignore_ascii_case("Content-Length") {
            if result.is_some() {
                return Err(Error::DuplicateContentLength);
            }
            let length = value
                .trim()
                .parse::<usize>()
                .map_err(|_| Error::InvalidHeader)?;
            if length > limits::JSON_BYTES {
                return Err(Error::MessageTooLarge);
            }
            result = Some(length);
        }
    }
    result.ok_or(Error::MissingContentLength)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fragmented_and_concatenated_frames_decode_exactly() {
        let first = encode(&json!({"jsonrpc":"2.0","id":1,"result":{}})).unwrap();
        let second = encode(&json!({"jsonrpc":"2.0","method":"initialized"})).unwrap();
        let split = first.len() - 3;
        let mut decoder = Decoder::default();
        assert!(decoder.push(&first[..split]).unwrap().is_empty());
        let mut tail = first[split..].to_vec();
        tail.extend(second);
        let values = decoder.push(&tail).unwrap();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0]["id"], 1);
        assert_eq!(values[1]["method"], "initialized");
    }

    #[test]
    fn duplicate_missing_and_oversized_lengths_fail_closed() {
        for bytes in [
            b"Other: 1\r\n\r\n{}".as_slice(),
            b"Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}".as_slice(),
        ] {
            assert!(Decoder::default().push(bytes).is_err());
        }
        let header = format!("Content-Length: {}\r\n\r\n", limits::JSON_BYTES + 1);
        assert!(matches!(
            Decoder::default().push(header.as_bytes()),
            Err(Error::MessageTooLarge)
        ));
    }

    #[test]
    fn a_partial_frame_survives_compact_checkpoint_serialization() {
        let message = encode(&json!({"jsonrpc":"2.0","id":1,"result":{"text":"å"}})).unwrap();
        let split = message.len() - 2;
        let mut decoder = Decoder::default();
        assert!(decoder.push(&message[..split]).unwrap().is_empty());
        let checkpoint = serde_json::to_string(&decoder).unwrap();
        assert!(!checkpoint.contains("[67,111,110"));
        let mut restored: Decoder = serde_json::from_str(&checkpoint).unwrap();
        let values = restored.push(&message[split..]).unwrap();
        assert_eq!(
            values,
            [json!({"jsonrpc":"2.0","id":1,"result":{"text":"å"}})]
        );
    }
}
