use super::PairingFailure;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

const SCHEME: &str = "soyeht-next://pair?";
/// The characters of a pairing code: 32 random bytes as URL-safe base64 without padding.
const CODE_LENGTH: usize = 43;
const FIELDS: [&str; 4] = ["engineId", "address", "code", "engineKey"];

/// What a pairing link carries. The address is a tailnet or loopback address and nothing else, so a
/// link in a message cannot send the phone's keys anywhere on the internet.
#[derive(Clone, PartialEq, Eq)]
pub struct PairingLink {
    pub engine_id: String,
    /// `host:port`, IPv6 in brackets.
    pub address: SocketAddr,
    pub(super) code: String,
    pub(super) engine_key: [u8; 32],
}

impl std::fmt::Debug for PairingLink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PairingLink")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl PairingLink {
    pub fn parse(text: &str) -> Result<Self, PairingFailure> {
        let text = text.trim();
        let query = text
            .strip_prefix(SCHEME)
            .ok_or(PairingFailure::InvalidLink)?;
        let mut fields: Vec<(String, String)> = Vec::new();
        for pair in query.split('&') {
            let (name, value) = pair.split_once('=').ok_or(PairingFailure::InvalidLink)?;
            fields.push((decode(name)?, decode(value)?));
        }
        let mut names: Vec<&str> = fields.iter().map(|(name, _)| name.as_str()).collect();
        names.sort_unstable();
        // A Mac that has not been updated writes the first three fields only.
        if names == ["address", "code", "engineId"] {
            return Err(PairingFailure::OldLink);
        }
        let mut expected = FIELDS;
        expected.sort_unstable();
        if names != expected {
            return Err(PairingFailure::InvalidLink);
        }
        let get = |name: &str| {
            fields
                .iter()
                .find(|(field, _)| field == name)
                .map(|(_, value)| value.as_str())
                .ok_or(PairingFailure::InvalidLink)
        };
        let engine_id = get("engineId")?;
        let code = get("code")?;
        if uuid::Uuid::parse_str(engine_id).is_err() || !is_key_text(code) {
            return Err(PairingFailure::InvalidLink);
        }
        let key_text = get("engineKey")?;
        let engine_key = is_key_text(key_text)
            .then(|| URL_SAFE_NO_PAD.decode(key_text).ok())
            .flatten()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(PairingFailure::InvalidLink)?;
        Ok(Self {
            engine_id: engine_id.to_owned(),
            address: permitted(get("address")?)?,
            code: code.to_owned(),
            engine_key,
        })
    }
}

fn is_key_text(text: &str) -> bool {
    text.len() == CODE_LENGTH
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// The address of a link: a literal IP and a port, on the tailnet (100.64.0.0/10, fd7a:115c:a1e0::/48)
/// or the loopback.
fn permitted(text: &str) -> Result<SocketAddr, PairingFailure> {
    let address: SocketAddr = text.parse().map_err(|_| PairingFailure::InvalidLink)?;
    let allowed = match address.ip() {
        IpAddr::V4(ip) => ip.is_loopback() || tailnet_v4(ip),
        IpAddr::V6(ip) => ip.is_loopback() || tailnet_v6(ip),
    };
    if allowed && address.port() != 0 {
        Ok(address)
    } else {
        Err(PairingFailure::InvalidLink)
    }
}

fn tailnet_v4(ip: Ipv4Addr) -> bool {
    let [first, second, ..] = ip.octets();
    first == 100 && (64..=127).contains(&second)
}

fn tailnet_v6(ip: Ipv6Addr) -> bool {
    ip.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0]
}

/// Percent-decoding of one query component.
fn decode(text: &str) -> Result<String, PairingFailure> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%' {
            let pair = bytes
                .get(index + 1..index + 3)
                .and_then(|pair| std::str::from_utf8(pair).ok())
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or(PairingFailure::InvalidLink)?;
            out.push(pair);
            index += 3;
        } else {
            out.push(byte);
            index += 1;
        }
    }
    String::from_utf8(out).map_err(|_| PairingFailure::InvalidLink)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENGINE: &str = "6f1e2d3c-4b5a-4978-8a6b-5c4d3e2f1a0b";
    const CODE: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const KEY: &str = "0EqyMnQrtKs6E2i9RhXk5tAiSrcaAWuvhSCjMsl3hzc";

    fn link(address: &str) -> String {
        format!(
            "soyeht-next://pair?engineId={ENGINE}&address={address}&code={CODE}&engineKey={KEY}"
        )
    }

    #[test]
    fn a_link_names_an_engine_a_tailnet_or_loopback_address_a_code_and_a_key() {
        let parsed = PairingLink::parse(&link("100.64.1.2:8443")).unwrap();
        assert_eq!(parsed.engine_id, ENGINE);
        assert_eq!(parsed.address.to_string(), "100.64.1.2:8443");
        assert_eq!(parsed.engine_key.len(), 32);
        assert!(PairingLink::parse(&link("127.0.0.1:9")).is_ok());
        assert!(PairingLink::parse(&link("%5Bfd7a:115c:a1e0::1%5D:9")).is_ok());
        assert!(PairingLink::parse(&link("100.64.1.2%3A8443")).is_ok());
    }

    #[test]
    fn an_address_outside_the_tailnet_is_refused() {
        for address in [
            "192.168.1.2:9",
            "8.8.8.8:9",
            "100.128.0.1:9",
            "example.com:9",
            "100.64.1.2:0",
            "100.64.1.2",
        ] {
            assert_eq!(
                PairingLink::parse(&link(address)).unwrap_err(),
                PairingFailure::InvalidLink,
                "{address}"
            );
        }
    }

    #[test]
    fn a_link_is_exactly_the_four_fields() {
        let old = format!("soyeht-next://pair?engineId={ENGINE}&address=127.0.0.1:9&code={CODE}");
        assert_eq!(
            PairingLink::parse(&old).unwrap_err(),
            PairingFailure::OldLink
        );
        for bad in [
            format!("{}&extra=1", link("127.0.0.1:9")),
            link("127.0.0.1:9").replace("soyeht-next", "https"),
            link("127.0.0.1:9").replace(ENGINE, "not-a-uuid"),
            link("127.0.0.1:9").replace(CODE, "short"),
            link("127.0.0.1:9").replace(KEY, CODE.replace('A', "!").as_str()),
            String::new(),
        ] {
            assert_eq!(
                PairingLink::parse(&bad).unwrap_err(),
                PairingFailure::InvalidLink,
                "{bad}"
            );
        }
    }
}
