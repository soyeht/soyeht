//! The pairing link a computer shows and a device opens (ADR 0013, 0129, 0132), and the addresses a
//! device may dial. The grammar is written here once: the engine writes the link, the Android core
//! reads it, and `contracts/vectors/pairing-link.json` holds the cases that this module and the Swift
//! client both pass.

use crate::{device_auth, limits::PAIRING_CODE_CHARS};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use std::net::{IpAddr, SocketAddr};

const PREFIX: &str = "soyeht-next://pair?";
const FIELDS: [&str; 4] = ["address", "code", "engineId", "engineKey"];

/// Why a text is not a pairing link a device can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    /// It is not a pairing link of this product.
    Invalid,
    /// It is the three-field link of an engine that does not prove itself (before ADR 0132).
    Old,
}

/// What a pairing link carries.
#[derive(Clone, PartialEq, Eq)]
pub struct PairingLink {
    pub engine_id: String,
    pub address: SocketAddr,
    pub code: String,
    /// The engine's Ed25519 public key.
    pub engine_key: [u8; 32],
}

impl std::fmt::Debug for PairingLink {
    /// The code and the key are what the link is for: neither is printed.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PairingLink")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

/// Listeners and links may name loopback and tailnet addresses only: never LAN, public or wildcard.
pub fn permitted_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            ip.is_loopback() || ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1])
        }
        IpAddr::V6(ip) => ip.is_loopback() || ip.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

/// The link of a pairing window. `address` is `host:port`, with an IPv6 host in brackets, and
/// `engine_key` the engine's public key as [`key_text`] writes it.
pub fn uri(engine_id: &str, address: &str, code: &str, engine_key: &str) -> String {
    format!("{PREFIX}engineId={engine_id}&address={address}&code={code}&engineKey={engine_key}")
}

/// A public key as a link carries it: URL-safe base64 without padding.
pub fn key_text(key: &[u8; 32]) -> String {
    URL_SAFE_NO_PAD.encode(key)
}

impl PairingLink {
    pub fn parse(text: &str) -> Result<Self, LinkError> {
        let query = text.trim().strip_prefix(PREFIX).ok_or(LinkError::Invalid)?;
        let mut fields: Vec<(String, String)> = Vec::new();
        for pair in query.split('&') {
            let (name, value) = pair.split_once('=').ok_or(LinkError::Invalid)?;
            fields.push((decode(name)?, decode(value)?));
        }
        let mut names: Vec<&str> = fields.iter().map(|(name, _)| name.as_str()).collect();
        names.sort_unstable();
        if names == ["address", "code", "engineId"] {
            return Err(LinkError::Old);
        }
        if names != FIELDS {
            return Err(LinkError::Invalid);
        }
        let get = |name: &str| {
            fields
                .iter()
                .find(|(field, _)| field == name)
                .map(|(_, value)| value.as_str())
                .ok_or(LinkError::Invalid)
        };
        let engine_id = get("engineId")?;
        let code = get("code")?;
        if uuid::Uuid::parse_str(engine_id).is_err() || !is_key_text(code) {
            return Err(LinkError::Invalid);
        }
        let key_text = get("engineKey")?;
        let engine_key: [u8; 32] = is_key_text(key_text)
            .then(|| URL_SAFE_NO_PAD.decode(key_text).ok())
            .flatten()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(LinkError::Invalid)?;
        // A key that signs for anyone is refused before any code is sent to its server.
        device_auth::verifying_key(&engine_key).ok_or(LinkError::Invalid)?;
        Ok(Self {
            engine_id: engine_id.to_owned(),
            address: address(get("address")?)?,
            code: code.to_owned(),
            engine_key,
        })
    }
}

fn is_key_text(text: &str) -> bool {
    text.len() == PAIRING_CODE_CHARS
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn address(text: &str) -> Result<SocketAddr, LinkError> {
    let address: SocketAddr = text.parse().map_err(|_| LinkError::Invalid)?;
    if permitted_address(address.ip()) && address.port() != 0 {
        Ok(address)
    } else {
        Err(LinkError::Invalid)
    }
}

/// Percent-decoding of one query component.
fn decode(text: &str) -> Result<String, LinkError> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%' {
            let value = bytes
                .get(index + 1..index + 3)
                .and_then(|pair| std::str::from_utf8(pair).ok())
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or(LinkError::Invalid)?;
            out.push(value);
            index += 3;
        } else {
            out.push(byte);
            index += 1;
        }
    }
    String::from_utf8(out).map_err(|_| LinkError::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTORS: &str = include_str!("../../../contracts/vectors/pairing-link.json");

    #[test]
    fn every_case_of_the_shared_vector_gives_the_same_answer() {
        let vector: serde_json::Value = serde_json::from_str(VECTORS).unwrap();
        let cases = vector["cases"].as_array().unwrap();
        assert!(cases.len() >= 10);
        for case in cases {
            let link = case["link"].as_str().unwrap();
            let found = PairingLink::parse(link);
            match case["result"].as_str().unwrap() {
                "valid" => {
                    let parsed =
                        found.unwrap_or_else(|error| panic!("{}: {error:?}", case["name"]));
                    assert_eq!(
                        parsed.address.to_string(),
                        case["address"].as_str().unwrap()
                    );
                    assert_eq!(parsed.engine_id, case["engineId"].as_str().unwrap());
                }
                "old" => assert_eq!(found.unwrap_err(), LinkError::Old, "{}", case["name"]),
                _ => assert_eq!(found.unwrap_err(), LinkError::Invalid, "{}", case["name"]),
            }
        }
    }

    #[test]
    fn the_link_the_engine_writes_is_a_link_a_device_reads() {
        let key = ed25519_dalek::SigningKey::from_bytes(&[7; 32])
            .verifying_key()
            .to_bytes();
        let link = uri(
            "6f1e2d3c-4b5a-4978-8a6b-5c4d3e2f1a0b",
            "100.64.1.2:8443",
            &"A".repeat(PAIRING_CODE_CHARS),
            &key_text(&key),
        );
        let parsed = PairingLink::parse(&link).unwrap();
        assert_eq!(parsed.engine_key, key);
        assert_eq!(parsed.address.to_string(), "100.64.1.2:8443");
    }

    #[test]
    fn only_loopback_and_tailnet_addresses_are_permitted() {
        for ip in [
            "127.0.0.1",
            "::1",
            "100.64.0.1",
            "100.127.255.255",
            "fd7a:115c:a1e0::1",
        ] {
            assert!(permitted_address(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "0.0.0.0",
            "192.168.1.2",
            "10.0.0.1",
            "8.8.8.8",
            "100.63.0.1",
            "100.128.0.1",
            "::",
        ] {
            assert!(!permitted_address(ip.parse().unwrap()), "{ip}");
        }
    }
}
