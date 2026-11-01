//! Reading the signed update manifest (ADR 0122, contract 15).
//!
//! The published file is an envelope that carries the payload and an Ed25519 signature over the
//! signing context followed by the payload's bytes. [`evaluate`] checks the size, the envelope,
//! the key, the signature and every rule of the payload, in that order, and then compares the
//! version on offer with the one running. It does no IO: the client fetches the text and decides
//! what to show. `contracts/vectors/update.json` holds the cases the Mac app (`UpdateCheck`) and
//! `scripts/release_manifest.py` pass as well, so the three readers cannot drift.

use crate::update_policy::{
    ARCHITECTURES, CHANNELS, Failure, MANIFEST_FORMAT, MANIFEST_MAX_BYTES, MAX_ADDRESS_BYTES,
    MAX_LIFETIME_SECONDS, MAX_PACKAGE_BYTES, MAX_PACKAGES, MAX_SEQUENCE, PublicKey,
    SHA256_HEX_LENGTH, SIGNING_CONTEXT, SYSTEMS, URL_PREFIXES, VERSION_PART_LIMIT,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;
use std::{collections::HashSet, fmt};

/// `MAJOR.MINOR.PATCH`: three plain numbers, none with a leading zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version([u32; 3]);

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let mut numbers = [0u32; 3];
        for number in &mut numbers {
            let part = parts.next()?;
            let plain = !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'));
            *number = part.parse().ok().filter(|_| plain)?;
            if *number >= VERSION_PART_LIMIT {
                return None;
            }
        }
        parts.next().is_none().then_some(Self(numbers))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.0[0], self.0[1], self.0[2])
    }
}

/// What a build is: the version it runs and the package it would take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Build<'a> {
    pub version: Version,
    /// As `std::env::consts::OS` spells it.
    pub system: &'a str,
    /// As `std::env::consts::ARCH` spells it.
    pub architecture: &'a str,
    pub channel: &'a str,
}

/// The package of a newer version for this system and architecture.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Package {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

/// A newer version and where to get it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub version: String,
    /// The running version is below the minimum supported one.
    pub required: bool,
    pub notes_url: String,
    pub package: Package,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    UpToDate,
    Available(Offer),
}

/// What a manifest that passed every check says, with its sequence number: the caller keeps the
/// highest one it has read and passes it back as the floor, so an older manifest is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub sequence: u64,
    pub outcome: Outcome,
}

#[derive(Deserialize)]
struct Envelope {
    format: serde_json::Value,
    #[serde(rename = "keyId")]
    key_id: String,
    payload: String,
    signature: String,
}

#[derive(Deserialize)]
struct Payload {
    format: serde_json::Value,
    channel: String,
    sequence: u64,
    /// Seconds since 1970 at which the manifest stops being read.
    #[serde(rename = "expiresAt")]
    expires_at: u64,
    version: String,
    #[serde(rename = "minimumSupportedVersion")]
    minimum: String,
    #[serde(rename = "releaseNotesUrl")]
    notes_url: String,
    packages: Vec<Listed>,
}

#[derive(Deserialize)]
struct Listed {
    system: String,
    architecture: String,
    #[serde(flatten)]
    package: Package,
}

/// Whether `value` is the whole number `expected` (a boolean or a text is not).
fn is_number(value: &serde_json::Value, expected: u32) -> bool {
    value.as_u64() == Some(u64::from(expected))
}

/// The bytes of padded standard base64, and nothing that merely decodes to them.
fn canonical_base64(text: &str) -> Option<Vec<u8>> {
    let Ok(raw) = STANDARD.decode(text) else {
        return None;
    };
    (STANDARD.encode(&raw) == text).then_some(raw)
}

/// `bytes` as an array, when it is exactly `N` long.
fn fixed<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
    (bytes.len() == N).then(|| {
        let mut array = [0u8; N];
        array.copy_from_slice(bytes);
        array
    })
}

/// A path segment that moves up or stays, in any spelling a browser or a server may read as one.
const DOT_SEGMENTS: [&str; 6] = [".", "..", "%2e", ".%2e", "%2e.", "%2e%2e"];

/// An address below an allowed prefix that names a file by a plain path: printable ASCII, no
/// backslash, no encoded slash or backslash, and no segment that moves up or stays.
fn is_address(value: &str) -> bool {
    if value.len() > MAX_ADDRESS_BYTES
        || !value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        || value.contains('\\')
        || !URL_PREFIXES.iter().any(|prefix| value.starts_with(prefix))
    {
        return false;
    }
    let path = value.split(['?', '#']).next().unwrap_or_default();
    let path = path.to_ascii_lowercase();
    !path.contains("%2f")
        && !path.contains("%5c")
        && !path
            .split('/')
            .any(|segment| DOT_SEGMENTS.contains(&segment))
}

fn is_sha256(value: &str) -> bool {
    value.len() == SHA256_HEX_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The payload bytes of `text` when a key of `keys` signed them.
fn open_envelope(text: &str, keys: &[PublicKey]) -> Result<Vec<u8>, Failure> {
    if keys.is_empty() {
        return Err(Failure::NotConfigured);
    }
    if text.len() > MANIFEST_MAX_BYTES {
        return Err(Failure::TooLarge);
    }
    let envelope: Envelope = serde_json::from_str(text).map_err(|_| Failure::BadEnvelope)?;
    if !is_number(&envelope.format, MANIFEST_FORMAT) {
        return Err(Failure::BadEnvelope);
    }
    let key = keys
        .iter()
        .find(|key| key.id == envelope.key_id)
        .ok_or(Failure::UnknownKey)?;
    let payload = canonical_base64(&envelope.payload).ok_or(Failure::BadEnvelope)?;
    let signature = canonical_base64(&envelope.signature)
        .and_then(|raw| fixed::<64>(&raw))
        .ok_or(Failure::BadEnvelope)?;
    // A key in the contract that is not 32 bytes of base64 never verifies anything.
    let Some(raw) = canonical_base64(key.key).and_then(|raw| fixed::<32>(&raw)) else {
        return Err(Failure::BadSignature);
    };
    // A key of small order verifies any message with the identity point as its signature; the
    // Mac's verifier does not refuse it, so every reader does.
    let Ok(public) = VerifyingKey::from_bytes(&raw) else {
        return Err(Failure::BadSignature);
    };
    if public.is_weak() {
        return Err(Failure::BadSignature);
    }
    let signed = [SIGNING_CONTEXT, &payload].concat();
    public
        .verify_strict(&signed, &Signature::from_bytes(&signature))
        .map_err(|_| Failure::BadSignature)?;
    Ok(payload)
}

/// The payload when it keeps every rule of the format.
fn check_payload(bytes: &[u8]) -> Result<(Payload, Version, Version), Failure> {
    let payload: Payload = serde_json::from_slice(bytes).map_err(|_| Failure::BadManifest)?;
    let version = Version::parse(&payload.version);
    let minimum = Version::parse(&payload.minimum);
    let (Some(version), Some(minimum)) = (version, minimum) else {
        return Err(Failure::BadManifest);
    };
    if !is_number(&payload.format, MANIFEST_FORMAT)
        || !CHANNELS.contains(&payload.channel.as_str())
        || minimum > version
        || payload.sequence == 0
        || payload.sequence > MAX_SEQUENCE
        || payload.expires_at <= payload.sequence
        || payload.expires_at > payload.sequence + MAX_LIFETIME_SECONDS
        || !is_address(&payload.notes_url)
        || payload.packages.is_empty()
        || payload.packages.len() > MAX_PACKAGES
    {
        return Err(Failure::BadManifest);
    }
    let mut seen = HashSet::new();
    for listed in &payload.packages {
        let package = &listed.package;
        if !seen.insert((listed.system.as_str(), listed.architecture.as_str()))
            || !SYSTEMS.contains(&listed.system.as_str())
            || !ARCHITECTURES.contains(&listed.architecture.as_str())
            || !is_address(&package.url)
            || !is_sha256(&package.sha256)
            || package.size == 0
            || package.size > MAX_PACKAGE_BYTES
        {
            return Err(Failure::BadManifest);
        }
    }
    Ok((payload, version, minimum))
}

/// What `build` should do with the manifest `text`, trusting `keys`. `floor` is the highest
/// sequence this device has read: a manifest below it is refused (`StaleManifest`). `now` is the
/// time in seconds since 1970: a manifest at or past its expiry is refused (`Expired`), so an old
/// one that someone keeps serving does not hold a device on an old release.
pub fn evaluate(
    text: &str,
    build: &Build<'_>,
    keys: &[PublicKey],
    floor: u64,
    now: u64,
) -> Result<Checked, Failure> {
    let (payload, version, minimum) = check_payload(&open_envelope(text, keys)?)?;
    if payload.channel != build.channel {
        return Err(Failure::WrongChannel);
    }
    if payload.sequence < floor {
        return Err(Failure::StaleManifest);
    }
    if now >= payload.expires_at {
        return Err(Failure::Expired);
    }
    let sequence = payload.sequence;
    if version <= build.version {
        return Ok(Checked {
            sequence,
            outcome: Outcome::UpToDate,
        });
    }
    let listed = payload
        .packages
        .into_iter()
        .find(|listed| listed.system == build.system && listed.architecture == build.architecture)
        .ok_or(Failure::NoPackage)?;
    Ok(Checked {
        sequence,
        outcome: Outcome::Available(Offer {
            version: payload.version,
            required: build.version < minimum,
            notes_url: payload.notes_url,
            package: listed.package,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const VECTORS: &str = include_str!("../../../contracts/vectors/update.json");

    fn vectors() -> Value {
        serde_json::from_str(VECTORS).unwrap()
    }

    /// The vectors' keys are not in the contract; `Box::leak` gives them the lifetime a
    /// `PublicKey` has in production.
    fn trusted(vectors: &Value, names: &Value) -> Vec<PublicKey> {
        names
            .as_array()
            .unwrap()
            .iter()
            .map(|name| {
                let key = &vectors["keys"][name.as_str().unwrap()];
                PublicKey {
                    id: Box::leak(key["id"].as_str().unwrap().to_owned().into_boxed_str()),
                    key: Box::leak(key["key"].as_str().unwrap().to_owned().into_boxed_str()),
                }
            })
            .collect()
    }

    fn recorded(result: &Result<Checked, Failure>) -> Value {
        match result
            .as_ref()
            .map(|checked| (checked.sequence, &checked.outcome))
        {
            Ok((sequence, Outcome::UpToDate)) => {
                serde_json::json!({"status": "upToDate", "sequence": sequence})
            }
            Ok((sequence, Outcome::Available(offer))) => serde_json::json!({
                "status": "available",
                "sequence": sequence,
                "version": offer.version,
                "required": offer.required,
                "notesUrl": offer.notes_url,
                "package": {
                    "url": offer.package.url,
                    "sha256": offer.package.sha256,
                    "size": offer.package.size,
                },
            }),
            Err(failure) => serde_json::json!({"status": "failed", "failure": failure.as_str()}),
        }
    }

    #[test]
    fn every_case_of_the_vectors_reads_as_recorded() {
        let vectors = vectors();
        let cases = vectors["cases"].as_array().unwrap();
        assert!(cases.len() > 60);
        for case in cases {
            let build = Build {
                version: Version::parse(case["running"].as_str().unwrap()).unwrap(),
                system: case["system"].as_str().unwrap(),
                architecture: case["architecture"].as_str().unwrap(),
                channel: case["channel"].as_str().unwrap(),
            };
            let keys = trusted(&vectors, &case["trust"]);
            let floor = case["floor"].as_u64().unwrap();
            let now = case["now"].as_u64().unwrap();
            let result = evaluate(
                case["manifest"].as_str().unwrap(),
                &build,
                &keys,
                floor,
                now,
            );
            assert_eq!(recorded(&result), case["expected"], "{}", case["name"]);
        }
    }

    #[test]
    fn a_build_that_carries_the_contracts_keys_trusts_no_test_key() {
        let vectors = vectors();
        let case = &vectors["cases"][0];
        let build = Build {
            version: Version::parse("0.1.0").unwrap(),
            system: "macos",
            architecture: "aarch64",
            channel: "stable",
        };
        let result = evaluate(
            case["manifest"].as_str().unwrap(),
            &build,
            crate::update_policy::PUBLIC_KEYS,
            0,
            0,
        );
        assert!(matches!(
            result,
            Err(Failure::NotConfigured | Failure::UnknownKey)
        ));
    }

    #[test]
    fn a_manifest_over_the_limit_is_refused_before_it_is_read() {
        let vectors = vectors();
        let keys = trusted(&vectors, &serde_json::json!(["first"]));
        let build = Build {
            version: Version::parse("0.1.0").unwrap(),
            system: "macos",
            architecture: "aarch64",
            channel: "stable",
        };
        let text = format!(
            "{}{}",
            vectors["cases"][0]["manifest"].as_str().unwrap(),
            " ".repeat(MANIFEST_MAX_BYTES)
        );
        assert_eq!(evaluate(&text, &build, &keys, 0, 0), Err(Failure::TooLarge));
    }

    #[test]
    fn versions_are_three_plain_numbers_that_compare_as_numbers() {
        for good in ["0.0.0", "0.1.0", "10.20.30", "999.999.999"] {
            assert_eq!(Version::parse(good).unwrap().to_string(), good);
        }
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "1.2.-3",
            "1.2.+3",
            "1.2. 3",
            "v1.2.3",
            "1.2.3-beta",
            "1000.0.0",
            "1.2.99999999999",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
        assert!(Version::parse("0.10.0") > Version::parse("0.9.0"));
        assert!(Version::parse("1.0.0") > Version::parse("0.999.999"));
    }

    #[test]
    fn the_failures_the_vectors_name_exist() {
        use Failure::*;
        let all = [
            NotConfigured,
            Unreachable,
            TooLarge,
            BadEnvelope,
            UnknownKey,
            BadSignature,
            BadManifest,
            WrongChannel,
            NoPackage,
            StaleManifest,
            Expired,
            UnknownBuild,
        ];
        let contract: Value =
            serde_json::from_str(include_str!("../../../contracts/update.json")).unwrap();
        let ids: Vec<&str> = contract["failures"]
            .as_array()
            .unwrap()
            .iter()
            .map(|failure| failure["id"].as_str().unwrap())
            .collect();
        assert_eq!(all.map(Failure::as_str).to_vec(), ids);
    }
}
