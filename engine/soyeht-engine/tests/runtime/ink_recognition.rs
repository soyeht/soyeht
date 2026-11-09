//! Handwriting recognition (ADR 0158, contract 14): `ink.recognize` is dispatched off the Core
//! lock, the limits of the contract are enforced, and `hello` tells whether the engine reads
//! handwriting.
use super::*;

/// How long the fixture recognizer takes to read. Long enough that a request which waited for it
/// would show, short enough to keep the suite quick.
const READING_MS: &str = "4000";
/// What the engine takes to answer a request that is not held back by a recognition, with a wide
/// margin for a loaded host; the recognition itself takes longer than this.
const PROMPT: Duration = Duration::from_millis(3_000);

fn stroke(points: usize) -> Value {
    Value::Array(
        (0..points)
            .map(|index| json!({"x": index as f64, "y": (index % 7) as f64, "t": index}))
            .collect(),
    )
}

fn writing(strokes: usize, points: usize) -> Value {
    json!({"strokes": vec![stroke(points); strokes], "languages": ["en-US"]})
}

fn refused(client: &mut Client, params: Value) -> Value {
    let response = client.request("ink.recognize", params);
    assert!(response.get("error").is_some(), "{response}");
    response["error"].clone()
}

#[test]
fn a_recognition_runs_off_the_core_lock_and_holds_back_nothing() {
    let rig = Rig::with_environment(&[("SOYEHT_TEST_INK_RECOGNIZER_MS", READING_MS)]);
    let mut writer = rig.client();
    let mut other = rig.client();
    assert!(
        writer
            .section("capabilities")
            .contains(&json!("ink-recognition"))
    );

    let started = Instant::now();
    let recognition = writer.send("ink.recognize", writing(2, 30));
    thread::sleep(Duration::from_millis(100));
    // Another connection takes the Core lock while the recognizer is still reading.
    let asked = Instant::now();
    other.call("engine.status", json!({}));
    assert!(asked.elapsed() < PROMPT, "{:?}", asked.elapsed());
    // The same connection is not held back either: the reply to the next request comes first.
    let behind = writer.send("engine.status", json!({}));
    writer.wait(behind);
    assert!(started.elapsed() < PROMPT, "{:?}", started.elapsed());

    let answer = writer.wait(recognition);
    assert!(
        started.elapsed() >= Duration::from_millis(3_900),
        "the recognizer did not read"
    );
    let result = &answer["result"];
    assert_eq!(result["recognizer"], "paused-fixture", "{answer}");
    assert_eq!(result["language"], "en-US");
    // The fixture says where it ran: no Core lock was held by its thread.
    assert_eq!(result["candidates"][0]["text"], "off-the-lock");
}

#[test]
fn a_request_over_a_limit_or_malformed_is_refused_before_any_recognition() {
    let rig = Rig::with_environment(&[("SOYEHT_TEST_INK_RECOGNIZER_MS", READING_MS)]);
    let mut client = rig.client();
    let started = Instant::now();
    for (what, params) in [
        ("strokes", writing(65, 3)),
        ("points of a stroke", writing(1, 2_001)),
        ("points in all", writing(5, 1_700)),
    ] {
        let error = refused(&mut client, params);
        assert_eq!(
            error["data"]["kind"], "ink_limit_exceeded",
            "{what}: {error}"
        );
        assert_eq!(error["code"], -32046, "{what}");
    }
    let mut many = writing(1, 3);
    many["maxCandidates"] = json!(11);
    assert_eq!(
        refused(&mut client, many)["data"]["kind"],
        "ink_limit_exceeded"
    );
    let error = refused(&mut client, json!({"strokes": [], "languages": ["en"]}));
    assert_eq!(error["data"]["kind"], "invalid_params");
    let error = refused(
        &mut client,
        json!({"strokes": [[{"x": 1}]], "languages": ["en"]}),
    );
    assert_eq!(error["data"]["kind"], "invalid_params");
    assert!(
        started.elapsed() < PROMPT,
        "an oversize request waited for a recognizer"
    );

    // A language the recognizer does not read is its own error, and it does not lie about the cause.
    let mut swedish = writing(1, 3);
    swedish["languages"] = json!(["sv-SE"]);
    let error = refused(&mut client, swedish);
    assert_eq!(error["data"]["kind"], "ink_language_unavailable");
    assert_eq!(error["code"], -32045);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn a_system_without_a_recognizer_says_so_and_offers_nothing() {
    let rig = Rig::new();
    let mut client = rig.client();
    assert!(
        !client
            .section("capabilities")
            .contains(&json!("ink-recognition"))
    );
    let error = refused(&mut client, writing(1, 3));
    assert_eq!(error["data"]["kind"], "ink_unsupported");
    assert_eq!(error["data"]["detail"], "no_os_support");
    assert_eq!(error["code"], -32044);
}

#[cfg(target_os = "macos")]
#[test]
fn the_mac_offers_vision_and_reports_a_language_it_does_not_read() {
    let rig = Rig::new();
    let mut client = rig.client();
    assert!(
        client
            .section("capabilities")
            .contains(&json!("ink-recognition"))
    );
    let mut klingon = writing(1, 3);
    klingon["languages"] = json!(["tlh"]);
    let error = refused(&mut client, klingon);
    assert_eq!(error["data"]["kind"], "ink_language_unavailable", "{error}");
}
