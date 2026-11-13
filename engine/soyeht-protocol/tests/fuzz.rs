//! Fuzz targets for everything in the protocol crate that reads bytes a peer chose: frames, the
//! greeting and `hello`, request parameters, NDJSON, attachments, snapshot pages, pairing links
//! and proofs, and the signed update manifest. Run by the gate with a fixed seed; the long run
//! is in docs/security-testing.md.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use soyeht_protocol::{
    HelloParams, Welcome,
    attachment::AttachmentAssembler,
    device_auth,
    frame::Frame,
    handshake, limits,
    ndjson::Decoder,
    pairing_link::{self, LinkError, PairingLink, permitted_address},
    snapshot::SnapshotAssembler,
    update::{self, Build, Outcome, Version},
    update_policy::{Failure, PublicKey},
    validate,
};
use std::io::{self, Read};

mod fuzz_support;
use fuzz_support::{Corpus, Rng, example, fuzz_bytes, fuzz_json, fuzz_text, hex};

/// Cases per target in the gate: a few seconds in all, from a fixed seed.
const CASES: u64 = 6_000;

fn encoded(frame: &Frame) -> Vec<u8> {
    let mut bytes = Vec::new();
    frame.write_to(&mut bytes).unwrap();
    bytes
}

fn frame_seeds() -> Vec<Vec<u8>> {
    vec![
        encoded(&Frame::Json(
            br#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{}}"#.to_vec(),
        )),
        encoded(&Frame::Terminal {
            pane_ref: 3,
            generation: 2,
            offset: u64::MAX,
            bytes: b"\x1b[31mred\x1b[0m".to_vec(),
        }),
        encoded(&Frame::Attachment {
            header: [7; 16],
            bytes: b"png".to_vec(),
        }),
        vec![limits::FRAME_KIND_JSON, 0xff, 0xff, 0xff, 0xff],
        vec![limits::FRAME_KIND_TERMINAL, 0, 0, 0, 15],
        vec![limits::FRAME_KIND_ATTACHMENT, 0x7f, 0xff, 0xff, 0xff],
        vec![0, 0, 0, 0, 1, 0],
        vec![limits::FRAME_KIND_JSON, 0, 0, 0, 0],
    ]
}

/// A reader that gives at most `step` bytes per call, the way a socket does.
struct Trickle<'a> {
    data: &'a [u8],
    step: usize,
}

impl Read for Trickle<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.step.min(buffer.len()).min(self.data.len());
        buffer[..count].copy_from_slice(&self.data[..count]);
        self.data = &self.data[count..];
        Ok(count)
    }
}

fn read_all(mut reader: impl Read) -> Vec<Result<Option<Frame>, String>> {
    let mut found = Vec::new();
    loop {
        match Frame::read_from(&mut reader) {
            Ok(Some(frame)) => found.push(Ok(Some(frame))),
            Ok(None) => return found,
            Err(error) => {
                found.push(Err(format!("{error:?}")));
                return found;
            }
        }
    }
}

#[test]
fn frames_from_any_bytes_stay_within_the_limits_and_read_the_same_in_any_chunking() {
    let seeds = frame_seeds();
    let seeds: Vec<&[u8]> = seeds.iter().map(Vec::as_slice).collect();
    let corpus = Corpus {
        seeds: &seeds,
        tokens: &[],
        max_len: 300,
    };
    fuzz_bytes("protocol-frames", CASES, &corpus, |input| {
        let whole = read_all(input);
        let trickled = read_all(Trickle {
            data: input,
            step: 1 + input.len() % 5,
        });
        assert_eq!(whole, trickled, "reading depends on how bytes arrive");
        let mut consumed = 0;
        for item in whole {
            let Ok(Some(frame)) = item else { break };
            let again = encoded(&frame);
            // What was read is exactly the bytes it came from, within the limits of its kind.
            assert_eq!(&input[consumed..consumed + again.len()], again.as_slice());
            let length = again.len() - 5;
            assert!(limits::JSON_BYTES >= length || !matches!(frame, Frame::Json(_)));
            consumed += again.len();
        }
    });
}

#[test]
fn an_announced_length_over_the_limit_is_refused_before_any_payload_is_read() {
    // Only the five header bytes exist: a reader that tried to take the body would fail with an
    // end of file rather than a length error, or allocate first.
    for (kind, over) in [
        (limits::FRAME_KIND_JSON, limits::JSON_BYTES + 1),
        (
            limits::FRAME_KIND_TERMINAL,
            limits::TERMINAL_HEADER_BYTES + limits::TERMINAL_PAYLOAD_BYTES + 1,
        ),
        (
            limits::FRAME_KIND_ATTACHMENT,
            limits::ATTACHMENT_HEADER_BYTES + limits::ATTACHMENT_PAYLOAD_BYTES + 1,
        ),
        (limits::FRAME_KIND_JSON, u32::MAX as usize),
    ] {
        let mut header = vec![kind];
        header.extend_from_slice(&(over as u32).to_be_bytes());
        let error = Frame::read_from(header.as_slice()).unwrap_err();
        assert!(
            matches!(error, soyeht_protocol::Error::InvalidFrameLength { .. }),
            "kind {kind} length {over}: {error:?}"
        );
    }
}

#[test]
fn ndjson_never_buffers_past_the_limit_and_splits_the_same_in_any_chunking() {
    let corpus = Corpus {
        seeds: &[
            b"{\"a\":1}\n{\"b\":2}\n",
            b"\n\n\n",
            b"no newline",
            b"{}\n{",
        ],
        tokens: &[b"\n", b"\r\n", b"{", b"\"", b"\xff"],
        max_len: 200,
    };
    fuzz_bytes("protocol-ndjson", CASES, &corpus, |input| {
        let mut whole = Decoder::new(7);
        let single = whole.push(7, input).unwrap().lines;
        let mut split = Decoder::new(7);
        let mut lines = Vec::new();
        let mut at = 0;
        let mut rng = Rng::new(input.len() as u64);
        while at < input.len() {
            let end = (at + 1 + rng.below(9)).min(input.len());
            let next = split.next_offset();
            lines.extend(split.push(next, &input[at..end]).unwrap().lines);
            assert!(split.buffered_len() <= limits::ACP_MESSAGE_BYTES);
            at = end;
        }
        assert_eq!(single, lines);
        let mut expected = 7;
        for line in &single {
            assert_eq!(line.from_seq, expected);
            assert_eq!(line.to_seq, expected + line.bytes.len() as u64 + 1);
            assert!(!line.bytes.contains(&b'\n'));
            expected = line.to_seq;
        }
    });
}

#[test]
fn ndjson_discards_an_oversized_record_through_its_newline_and_keeps_the_next() {
    let mut decoder = Decoder::new(0);
    let long = vec![b'x'; limits::ACP_MESSAGE_BYTES * 2 + 5];
    let batch = decoder.push(0, &long).unwrap();
    assert!(batch.lines.is_empty() && batch.oversized);
    assert!(decoder.buffered_len() <= limits::ACP_MESSAGE_BYTES);
    let next = decoder.next_offset();
    let batch = decoder.push(next, b"\n{\"ok\":true}\n").unwrap();
    assert_eq!(batch.lines.len(), 1);
    assert_eq!(batch.lines[0].bytes, br#"{"ok":true}"#);
}

fn hello_value() -> Value {
    json!({"client":"app","clientVersion":"1","protocolVersion":1,
        "deviceToken":"t","proof":"AAAA","clientNonce":"AAAA","binding":"b",
        "capabilities":["web_host"]})
}

fn welcome_value() -> Value {
    json!({"engineVersion":"1","protocolVersion":1,"connectionId":"c","role":"human_local",
        "capabilities":[],"dbId":"d"})
}

#[test]
fn the_greeting_and_hello_answers_decode_or_fail_without_a_panic_and_grant_only_the_asked_role() {
    let seeds = [
        json!({"method":"auth.challenge","params":{"nonce":"AA==","engineId":"e"}}),
        hello_value(),
        welcome_value(),
    ];
    fuzz_json("protocol-handshake", CASES, &seeds, |value| {
        let _ = handshake::expect_challenge(value);
        let _ = serde_json::from_value::<HelloParams>(value.clone());
        if let Ok(welcome) = serde_json::from_value::<Welcome>(value.clone()) {
            for bound in [false, true] {
                if handshake::expect_welcome(&welcome, bound).is_ok() {
                    // A welcome is accepted only for a version in the range and a role that matches
                    // whether the client asked for a binding.
                    assert!(
                        (limits::MIN_PROTOCOL_VERSION..=limits::PROTOCOL_VERSION)
                            .contains(&welcome.protocol_version)
                    );
                    assert_eq!(
                        bound,
                        matches!(
                            welcome.role,
                            soyeht_protocol::Role::Agent | soyeht_protocol::Role::Chief
                        )
                    );
                }
            }
            if handshake::expect_device_welcome(&welcome).is_ok() {
                assert_eq!(welcome.role, soyeht_protocol::Role::HumanDevice);
            }
        }
    });
}

fn method_names() -> Vec<String> {
    let schema: Value = serde_json::from_str(soyeht_protocol::SCHEMA).unwrap();
    schema["x-methods"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

#[test]
fn request_parameters_of_every_method_are_validated_without_a_panic_and_round_trip() {
    let schema: Value = serde_json::from_str(soyeht_protocol::SCHEMA).unwrap();
    let methods = method_names();
    assert!(methods.len() > 50);
    let unusable: Vec<&String> = methods
        .iter()
        .filter(|method| {
            validate::parameters(
                method,
                &example(&schema, &schema["x-methods"][method.as_str()]["params"], 0),
            )
            .is_err()
        })
        .collect();
    // The seeds of the mutation are valid: a mutation starts one step from a real request.
    assert!(
        unusable.is_empty(),
        "examples that do not validate: {unusable:?}"
    );
    let seeds: Vec<Value> = methods
        .iter()
        .map(|method| {
            let params = example(&schema, &schema["x-methods"][method]["params"], 0);
            json!({"method": method, "params": params})
        })
        .collect();
    fuzz_json("protocol-parameters", CASES * 2, &seeds, |case| {
        let method = case["method"].as_str().unwrap_or("hello");
        let params = &case["params"];
        let first = validate::parameters(method, params);
        let _ = validate::result(method, params);
        if let Ok(request) = first {
            // Accepted parameters are accepted again after the typed model writes them back:
            // the schema check and the model agree on what a request is.
            let written = serde_json::to_value(&request).unwrap();
            assert!(
                validate::parameters(method, &written["params"]).is_ok(),
                "{method}: {params} became {written}"
            );
        }
    });
    // A method the contract does not name is refused, whatever its parameters.
    assert!(validate::parameters("no.such.method", &json!({})).is_err());
}

#[test]
fn unknown_and_ill_typed_parameters_are_refused_for_every_method() {
    let schema: Value = serde_json::from_str(soyeht_protocol::SCHEMA).unwrap();
    for method in method_names() {
        for bad in [json!(null), json!(1), json!("x"), json!([])] {
            assert!(
                validate::parameters(&method, &bad).is_err(),
                "{method} took {bad}"
            );
        }
        let _ = example(&schema, &schema["x-methods"][&method]["params"], 0);
    }
}

fn signing_key(seed: u8) -> ed25519_dalek::SigningKey {
    ed25519_dalek::SigningKey::from_bytes(&[seed; 32])
}

fn link(seed: u8, address: &str) -> String {
    pairing_link::uri(
        "6f1e2d3c-4b5a-4978-8a6b-5c4d3e2f1a0b",
        address,
        &"A".repeat(limits::PAIRING_CODE_CHARS),
        &pairing_link::key_text(&signing_key(seed).verifying_key().to_bytes()),
    )
}

#[test]
fn a_pairing_link_that_parses_names_only_a_loopback_or_tailnet_address_and_round_trips() {
    let valid = [
        link(7, "100.64.1.2:8443"),
        link(9, "127.0.0.1:1"),
        link(3, "[fd7a:115c:a1e0::1]:9"),
    ];
    let seeds: Vec<&[u8]> = valid.iter().map(String::as_bytes).collect();
    let corpus = Corpus {
        seeds: &seeds,
        tokens: &[
            b"%",
            b"%00",
            b"%2f",
            b"&",
            b"=",
            b"@",
            b"[",
            b"]",
            b"0.0.0.0",
            b"192.168.1.1",
            b"::ffff:10.0.0.1",
            b"engineId=",
            b"address=",
            b"code=",
            b"engineKey=",
            b":0",
            b"soyeht-next://pair?",
            b"\n",
        ],
        max_len: 400,
    };
    fuzz_text(
        "protocol-pairing-link",
        CASES * 3,
        &corpus,
        |text| match PairingLink::parse(text) {
            Ok(parsed) => {
                assert!(permitted_address(parsed.address.ip()), "{text:?}");
                assert_ne!(parsed.address.port(), 0);
                assert_eq!(parsed.code.len(), limits::PAIRING_CODE_CHARS);
                // Any spelling of a UUID the parser reads (hyphenated, simple, braced, urn) is taken as the
                // engine id; a client compares it as text with the greeting's and fails closed.
                assert!(uuid::Uuid::parse_str(&parsed.engine_id).is_ok());
                assert!(device_auth::verifying_key(&parsed.engine_key).is_some());
                let written = pairing_link::uri(
                    &parsed.engine_id,
                    &parsed.address.to_string(),
                    &parsed.code,
                    &pairing_link::key_text(&parsed.engine_key),
                );
                assert_eq!(PairingLink::parse(&written).unwrap(), parsed);
            }
            Err(LinkError::Invalid | LinkError::Old) => {}
        },
    );
}

#[test]
fn a_link_naming_its_engine_in_another_spelling_of_a_uuid_is_read_and_never_matches_the_greeting() {
    // Found by the long run: the engine id of a link may be any spelling `Uuid` reads. The
    // greeting names the engine hyphenated, and a client compares the two as text.
    let hyphenated = "6f1e2d3c-4b5a-4978-8a6b-5c4d3e2f1a0b";
    let simple = hyphenated.replace('-', "");
    let good = link(7, "100.64.1.2:8443");
    let other = good.replace(hyphenated, &simple);
    let parsed = PairingLink::parse(&other).unwrap();
    assert_eq!(parsed.engine_id, simple);
    assert_ne!(parsed.engine_id, hyphenated);
}

#[test]
fn a_public_key_of_small_order_or_in_a_non_canonical_form_is_never_accepted() {
    // The identity, the order-2 point, the order-4 points and the non-canonical spellings of
    // y = 0 and y = 1 (p and p + 1): each signs for anyone, so no side may take it.
    let mut small = vec![[0u8; 32], {
        let mut one = [0u8; 32];
        one[0] = 1;
        one
    }];
    let mut p = [0xffu8; 32];
    p[0] = 0xed;
    p[31] = 0x7f;
    small.push(p);
    let mut p_plus_one = p;
    p_plus_one[0] = 0xee;
    small.push(p_plus_one);
    let mut order_two = [0u8; 32];
    order_two[0] = 0xec;
    order_two[1..31].fill(0xff);
    order_two[31] = 0x7f;
    small.push(order_two);
    for key in small {
        assert!(device_auth::verifying_key(&key).is_none(), "{}", hex(&key));
    }
    let corpus = Corpus {
        seeds: &[&[0u8; 32], &[0xffu8; 32], &[1u8; 32]],
        tokens: &[],
        max_len: 32,
    };
    fuzz_bytes("protocol-verifying-key", CASES, &corpus, |bytes| {
        let Ok(key) = <[u8; 32]>::try_from(bytes) else {
            return;
        };
        if let Some(verifying) = device_auth::verifying_key(&key) {
            assert!(!verifying.is_weak());
            assert_eq!(verifying.to_bytes(), key);
        }
    });
}

#[test]
fn a_pairing_proof_matches_only_the_exact_code_key_challenge_and_name() {
    let (key, challenge) = ([4u8; 32], [5u8; 32]);
    let good = device_auth::pair_proof("code", &key, &challenge, "name").unwrap();
    assert!(device_auth::pair_proof_matches(
        "code", &key, &challenge, "name", &good
    ));
    // Replayed against another challenge, another key, another name or another code: refused.
    assert!(!device_auth::pair_proof_matches(
        "code", &key, &[6u8; 32], "name", &good
    ));
    assert!(!device_auth::pair_proof_matches(
        "code", &[8u8; 32], &challenge, "name", &good
    ));
    assert!(!device_auth::pair_proof_matches(
        "code", &key, &challenge, "other", &good
    ));
    assert!(!device_auth::pair_proof_matches(
        "other", &key, &challenge, "name", &good
    ));
    let corpus = Corpus {
        seeds: &[good.as_bytes(), b"", b"AAAA", b"====", b"!!!!"],
        tokens: &[b"=", b"A", b"/", b"+", b"-", b"_"],
        max_len: 90,
    };
    fuzz_text("protocol-pair-proof", CASES, &corpus, |proof| {
        let accepted = device_auth::pair_proof_matches("code", &key, &challenge, "name", proof);
        // The proof is accepted only when it decodes to the one MAC, not for a forgery or a
        // different spelling that decodes to something else.
        if accepted {
            use base64::{Engine as _, engine::general_purpose::STANDARD};
            assert_eq!(STANDARD.decode(proof), STANDARD.decode(&good));
        }
    });
}

#[test]
fn attachment_frames_are_assembled_within_the_declared_limit_and_never_panic() {
    let corpus = Corpus {
        seeds: &[&[1, 2, 3, 4], &[0, 0, 0, 0]],
        tokens: &[],
        max_len: 80,
    };
    fuzz_bytes("protocol-attachments", CASES, &corpus, |input| {
        let mut assembler = AttachmentAssembler::new(64).unwrap();
        let mut rng = Rng::new(input.len() as u64 + 1);
        for chunk in input.chunks(1 + input.len() % 11) {
            let header = [chunk[0] % 3; 16];
            let _ = assembler.push(header, &chunk[1..]);
            if rng.chance(5) {
                let _ = assembler.commit_all(&[]);
                // Every outcome leaves the assembler empty for the next request.
                assert!(!assembler.is_pending());
            }
        }
        assembler.discard();
        assert!(!assembler.is_pending());
    });
}

#[test]
fn snapshot_pages_that_skip_mix_or_miscount_are_refused() {
    let page =
        |index: u32, of: u32, id: &str, data: Value| -> soyeht_protocol::StateSnapshotPartEvent {
            serde_json::from_value(json!({
                "snapshotId": id, "index": index, "of": of, "section": "panes", "data": data
            }))
            .unwrap()
        };
    let seeds = [
        json!([0, 2, "a", []]),
        json!([1, 2, "a", [1]]),
        json!([5, 2, "b", {}]),
    ];
    fuzz_json("protocol-snapshot", CASES, &seeds, |case| {
        let (Some(index), Some(of), Some(id)) =
            (case[0].as_u64(), case[1].as_u64(), case[2].as_str())
        else {
            return;
        };
        let mut assembler = SnapshotAssembler::default();
        let _ = assembler.push(page(index as u32, of as u32, id, case[3].clone()));
        let second = assembler.push(page(1, of as u32, "other", json!([])));
        // A second page from another snapshot is never taken into the first.
        assert!(second.is_err() || id == "other");
    });
}

fn manifest_cases() -> Vec<Value> {
    let vectors: Value =
        serde_json::from_str(include_str!("../../../contracts/vectors/update.json")).unwrap();
    vectors["cases"].as_array().unwrap().clone()
}

/// A string that lives for the whole run, as a key of the contract does in production.
fn forever(text: &Value) -> &'static str {
    Box::leak(text.as_str().unwrap().to_owned().into_boxed_str())
}

/// What one case of the shared vector reads a manifest with: its keys, its build, its floor and
/// its clock.
struct Reader<'a> {
    keys: Vec<PublicKey>,
    build: Build<'a>,
    floor: u64,
    now: u64,
}

impl<'a> Reader<'a> {
    fn new(vectors: &Value, case: &'a Value) -> Self {
        let mut keys = Vec::new();
        for name in case["trust"].as_array().unwrap() {
            let key = &vectors["keys"][name.as_str().unwrap()];
            keys.push(PublicKey {
                id: forever(&key["id"]),
                key: forever(&key["key"]),
            });
        }
        let version = Version::parse(case["running"].as_str().unwrap()).unwrap();
        Self {
            keys,
            build: Build {
                version,
                system: case["system"].as_str().unwrap(),
                architecture: case["architecture"].as_str().unwrap(),
                channel: case["channel"].as_str().unwrap(),
            },
            floor: case["floor"].as_u64().unwrap(),
            now: case["now"].as_u64().unwrap(),
        }
    }

    fn read(&self, manifest: &str) -> Result<update::Checked, Failure> {
        update::evaluate(manifest, &self.build, &self.keys, self.floor, self.now)
    }
}

#[test]
fn a_changed_update_manifest_is_never_accepted_as_a_different_offer() {
    let vectors: Value =
        serde_json::from_str(include_str!("../../../contracts/vectors/update.json")).unwrap();
    let cases = manifest_cases();
    // Every case of the vector is read without a panic by its own build.
    for case in &cases {
        let _ = Reader::new(&vectors, case).read(case["manifest"].as_str().unwrap());
    }
    // The first case the vector accepts is the seed; every mutation is read as its build.
    let case = cases
        .iter()
        .find(|case| case["expected"]["status"] == "available")
        .unwrap();
    let original = case["manifest"].as_str().unwrap();
    let reader = Reader::new(&vectors, case);
    let reference = reader.read(original);
    assert!(matches!(
        reference,
        Ok(update::Checked {
            outcome: Outcome::Available(_),
            ..
        })
    ));
    let seeds = [original.as_bytes()];
    let corpus = Corpus {
        seeds: &seeds,
        tokens: &[
            b"\"payload\"",
            b"\"signature\"",
            b"\"keyId\"",
            b"test-weak",
            b"AAAA",
            b"../",
        ],
        max_len: original.len() + 64,
    };
    fuzz_text("protocol-update-manifest", CASES, &corpus, |text| {
        if let Ok(found) = reader.read(text) {
            // Any spelling that is accepted says exactly what the signed payload says.
            assert_eq!(Ok(found), reference);
        }
    });
}

#[test]
fn a_version_is_three_plain_numbers_and_nothing_else() {
    let corpus = Corpus {
        seeds: &[b"1.2.3", b"0.0.0", b"01.2.3", b"1.2", b"1.2.3.4"],
        tokens: &[b".", b"0", b"9", b"-", b"+", b" "],
        max_len: 24,
    };
    fuzz_text("protocol-version", CASES, &corpus, |text| {
        if let Some(version) = Version::parse(text) {
            assert_eq!(version.to_string(), text);
        }
    });
}
