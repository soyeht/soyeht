use super::*;
use crate::testing::{cases, text};
use serde_json::json;
use soyeht_protocol::{ErrorData, EventName, RpcError};

const FINGERPRINT: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const COMPARISON: &str = "E3B0 C442 98FC 1C14";

fn offer(expires_at: f64) -> Offer {
    Offer {
        matrix: Matrix::encode("SOYEHT-PAIR 1 engineId=x"),
        link: "soyeht-next://pair?engineId=x".to_owned(),
        expires_at,
        problem: None,
    }
}

fn refusal(kind: ErrorKind, message: &str) -> Refusal {
    Refusal::of(&Error::Remote(RpcError {
        code: -32000,
        message: message.to_owned(),
        data: ErrorData {
            kind,
            detail: None,
            retry_after_sec: None,
        },
    }))
}

/// An engine that asks for the person's code, or one that does not.
fn adopted(code: bool) -> Pairing {
    let mut pairing = Pairing::default();
    let capabilities = if code {
        vec![json!("terminal"), json!("pairing-code")]
    } else {
        vec![json!("terminal")]
    };
    assert_eq!(pairing.adopt(&capabilities, Vec::new(), &[], true), None);
    pairing
}

/// A sheet that shows a window, with `ticket` as the attempt that opened it.
fn showing(pairing: &mut Pairing, expires_at: f64) {
    let Some(Command::Start { ticket }) = pairing.open(true) else {
        panic!("a connected sheet asks for a window");
    };
    pairing.apply(Event::Started {
        ticket,
        offer: offer(expires_at),
    });
}

fn requested(pairing: &mut Pairing, id: &str, fingerprint: &str) {
    let handled = pairing
        .notified(
            &EventName::DevicePairRequested,
            json!({"eventId": "9", "at": "now", "requestId": id,
                "deviceName": "Caio's phone", "fingerprint": fingerprint}),
        )
        .unwrap();
    assert!(handled);
}

fn device(id: &str) -> Device {
    Device {
        device_id: id.to_owned(),
        name: "Phone".to_owned(),
        fingerprint: FINGERPRINT.to_owned(),
        revoked: false,
    }
}

fn typed(command: Command) -> Option<String> {
    match command {
        Command::Decide { code, .. } => code.map(|code| code.0),
        other => panic!("not a decision: {other:?}"),
    }
}

#[test]
fn the_sheet_asks_for_a_window_and_shows_the_answer_of_its_own_attempt() {
    let mut pairing = adopted(true);
    assert!(!pairing.shown());
    let Some(Command::Start { ticket }) = pairing.open(true) else {
        panic!("a window is asked for");
    };
    assert_eq!(pairing.phase(), &Phase::Starting { ticket });
    // The answer of an attempt that is not this one changes nothing.
    pairing.apply(Event::Started {
        ticket: ticket + 7,
        offer: offer(100.0),
    });
    assert_eq!(pairing.phase(), &Phase::Starting { ticket });
    pairing.apply(Event::Started {
        ticket,
        offer: offer(100.0),
    });
    assert!(matches!(pairing.phase(), Phase::Showing(_)));
    // A sheet that was closed in the meantime stays closed when the answer comes.
    pairing.close();
    pairing.apply(Event::Started {
        ticket,
        offer: offer(100.0),
    });
    assert!(!pairing.shown());
}

#[test]
fn a_wrong_code_says_how_many_tries_are_left_and_the_right_one_is_sent_and_allows() {
    let mut pairing = adopted(true);
    showing(&mut pairing, 1000.0);
    requested(&mut pairing, "r1", FINGERPRINT);
    assert_eq!(
        pairing.requests()[0].comparison.as_deref(),
        Some(COMPARISON)
    );

    // Digits the phone shows, typed with the blank it groups them with.
    let command = pairing.decide("r1", true, "123 456").unwrap();
    assert_eq!(typed(command), Some("123456".to_owned()));
    assert!(pairing.requests()[0].deciding);
    // The answer is awaited: a second click sends nothing.
    assert_eq!(
        pairing.decide("r1", true, "123456"),
        Err(Rejected::Unavailable)
    );

    pairing.apply(Event::DecideFailed {
        request_id: "r1".to_owned(),
        refusal: refusal(
            ErrorKind::PairingCodeInvalid,
            "That code is not right. 2 tries left",
        ),
    });
    let request = &pairing.requests()[0];
    assert!(!request.deciding && !request.closed);
    assert_eq!(
        request.note.as_deref(),
        Some("That code is not right. 2 tries left")
    );

    let command = pairing.decide("r1", true, "654321").unwrap();
    assert_eq!(typed(command), Some("654321".to_owned()));
    assert_eq!(pairing.requests()[0].note, None);
    pairing.apply(Event::Decided {
        request_id: "r1".to_owned(),
        approved: true,
    });
    assert!(pairing.requests().is_empty());
    assert!(!pairing.shown(), "the sheet has done its work");
}

#[test]
fn a_text_that_is_not_six_digits_is_refused_here_without_a_call() {
    let mut pairing = adopted(true);
    showing(&mut pairing, 1000.0);
    requested(&mut pairing, "r1", FINGERPRINT);
    for input in ["", "12345", "1234567", "12345a", "123-456", "123\t456"] {
        assert_eq!(
            pairing.decide("r1", true, input),
            Err(Rejected::MalformedCode),
            "{input:?}"
        );
        let request = &pairing.requests()[0];
        assert!(!request.deciding, "nothing is awaited for {input:?}");
        assert_eq!(request.note.as_deref(), Some(copy::MALFORMED_CODE));
    }
}

#[test]
fn the_shared_vector_decides_what_is_a_code() {
    let cases = cases("pairing-approval", "cases");
    assert!(cases.len() >= 10);
    for case in cases {
        let mut pairing = adopted(true);
        requested(&mut pairing, "r1", FINGERPRINT);
        let decision = pairing.decide("r1", true, text(&case["input"]));
        match case["digits"].as_str() {
            Some(digits) => assert_eq!(
                typed(decision.unwrap()),
                Some(digits.to_owned()),
                "{}",
                case["name"]
            ),
            None => assert_eq!(decision, Err(Rejected::MalformedCode), "{}", case["name"]),
        }
    }
}

#[test]
fn the_comparison_code_is_the_shared_vectors() {
    for case in cases("pairing-comparison", "cases") {
        let mut pairing = Pairing::default();
        requested(&mut pairing, "r1", text(&case["fingerprint"]));
        assert_eq!(
            pairing.requests()[0].comparison.as_deref(),
            case["code"].as_str(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn no_code_is_sent_to_an_engine_that_does_not_ask_for_one_nor_with_a_denial() {
    let mut mac = adopted(false);
    requested(&mut mac, "r1", FINGERPRINT);
    // Whatever is in the field, an engine with no `pairing-code` is sent none, and a text that is
    // not a code does not stop the person from allowing.
    assert_eq!(typed(mac.decide("r1", true, "123456").unwrap()), None);

    let mut linux = adopted(true);
    requested(&mut linux, "r1", FINGERPRINT);
    assert_eq!(typed(linux.decide("r1", false, "123456").unwrap()), None);
    // A denial needs no code at all.
    let mut other = adopted(true);
    requested(&mut other, "r1", FINGERPRINT);
    assert!(other.decide("r1", false, "").is_ok());
}

#[test]
fn a_request_with_no_comparison_code_can_only_be_denied() {
    let mut pairing = adopted(true);
    requested(&mut pairing, "r1", "e3b0");
    assert_eq!(pairing.requests()[0].comparison, None);
    assert_eq!(
        pairing.decide("r1", true, "123456"),
        Err(Rejected::NoComparison)
    );
    assert!(pairing.decide("r1", false, "").is_ok());
}

#[test]
fn a_request_the_engine_no_longer_holds_is_over() {
    for kind in [
        ErrorKind::TransitionRefused,
        ErrorKind::NotFound,
        ErrorKind::Unauthorized,
    ] {
        let mut pairing = adopted(true);
        requested(&mut pairing, "r1", FINGERPRINT);
        pairing.decide("r1", true, "123456").unwrap();
        pairing.apply(Event::DecideFailed {
            request_id: "r1".to_owned(),
            refusal: refusal(kind, "gone"),
        });
        let request = &pairing.requests()[0];
        assert!(request.closed && !request.deciding);
        assert_eq!(request.note.as_deref(), Some(copy::REQUEST_GONE));
        assert_eq!(pairing.decide("r1", false, ""), Err(Rejected::Unavailable));
        pairing.dismiss("r1");
        assert!(pairing.requests().is_empty());
    }
    // A request still waiting is not dismissed by accident.
    let mut pairing = adopted(true);
    requested(&mut pairing, "r1", FINGERPRINT);
    pairing.dismiss("r1");
    assert_eq!(pairing.requests().len(), 1);
}

#[test]
fn a_denial_removes_the_request_and_keeps_the_sheet_open() {
    let mut pairing = adopted(true);
    showing(&mut pairing, 1000.0);
    requested(&mut pairing, "r1", FINGERPRINT);
    pairing.decide("r1", false, "").unwrap();
    pairing.apply(Event::Decided {
        request_id: "r1".to_owned(),
        approved: false,
    });
    assert!(pairing.requests().is_empty());
    assert!(matches!(pairing.phase(), Phase::Showing(_)));
}

#[test]
fn the_window_runs_out_and_the_time_left_counts_down() {
    let mut pairing = adopted(true);
    showing(&mut pairing, 100.0);
    assert_eq!(pairing.remaining(40.0), Some(60));
    assert_eq!(pairing.remaining(99.2), Some(1));
    assert!(!pairing.tick(99.9));
    assert!(pairing.tick(100.0));
    assert_eq!(pairing.phase(), &Phase::Expired);
    assert_eq!(pairing.remaining(100.0), None);
    assert!(!pairing.tick(101.0), "it does not change twice");
    // Asking again opens a new window.
    assert!(matches!(pairing.open(true), Some(Command::Start { .. })));
}

#[test]
fn losing_the_connection_loses_the_window_and_the_next_connection_starts_again() {
    let mut pairing = adopted(true);
    showing(&mut pairing, 1000.0);
    requested(&mut pairing, "r1", FINGERPRINT);
    let Phase::Showing(_) = pairing.phase() else {
        panic!("shown")
    };
    pairing.connection_lost();
    assert_eq!(pairing.phase(), &Phase::Interrupted);
    assert!(
        pairing.requests().is_empty(),
        "the window took them with it"
    );
    assert_eq!(pairing.decide("r1", false, ""), Err(Rejected::Unavailable));

    // The snapshot of the new connection: the sheet asks for a window again, with a new ticket.
    let restart = pairing.adopt(&[json!("pairing-code")], Vec::new(), &[], true);
    let Some(Command::Start { ticket }) = restart else {
        panic!("the pairing restarts");
    };
    assert_eq!(pairing.phase(), &Phase::Starting { ticket });
    assert!(ticket > 1);
}

#[test]
fn a_closed_sheet_does_not_start_a_pairing_when_the_connection_returns() {
    let mut pairing = adopted(true);
    pairing.connection_lost();
    assert_eq!(pairing.phase(), &Phase::Closed);
    assert_eq!(pairing.adopt(&[], Vec::new(), &[], true), None);
    assert!(!pairing.offers_code(), "the new engine does not offer it");
}

#[test]
fn a_sheet_opened_with_no_connection_waits_for_one() {
    let mut pairing = adopted(true);
    assert_eq!(pairing.open(false), None);
    assert_eq!(pairing.phase(), &Phase::Interrupted);
    assert!(matches!(
        pairing.adopt(&[], Vec::new(), &[], true),
        Some(Command::Start { .. })
    ));
}

#[test]
fn an_engine_that_refuses_is_shown_with_its_words_instead_of_a_code() {
    let mut pairing = adopted(false);
    let Some(Command::Start { ticket }) = pairing.open(true) else {
        panic!("asks")
    };
    let words = "Pairing is started and approved only in the signed Soyeht app";
    pairing.apply(Event::StartFailed {
        ticket,
        refusal: refusal(ErrorKind::ForbiddenRole, words),
    });
    let Phase::Refused(refused) = pairing.phase() else {
        panic!("refused");
    };
    assert_eq!(refused.message, words);
    assert_eq!(refused.kind, Some(ErrorKind::ForbiddenRole));
    // The person can ask again once the engine can.
    assert!(matches!(pairing.open(true), Some(Command::Start { .. })));
}

#[test]
fn a_request_that_comes_in_with_the_sheet_closed_waits_and_can_be_denied() {
    let mut pairing = adopted(true);
    showing(&mut pairing, 1000.0);
    pairing.close();
    requested(&mut pairing, "r1", FINGERPRINT);
    assert!(!pairing.shown());
    assert_eq!(pairing.requests().len(), 1);
    assert!(pairing.decide("r1", false, "").is_ok());
}

#[test]
fn a_request_is_taken_for_the_device_when_it_is_paired_and_a_revoked_one_is_marked() {
    let mut pairing = adopted(true);
    requested(&mut pairing, "r1", FINGERPRINT);
    let paired = json!({"eventId": "10", "at": "now", "device": {
        "deviceId": "d1", "name": "Phone", "fingerprint": FINGERPRINT, "revoked": false}});
    assert!(pairing.notified(&EventName::DevicePaired, paired).unwrap());
    assert!(pairing.requests().is_empty());
    assert_eq!(pairing.devices(), &[device("d1")]);

    pairing.ask_to_revoke("d1");
    assert_eq!(pairing.revoking().map(|d| d.device_id.as_str()), Some("d1"));
    pairing.cancel_revoke();
    assert!(pairing.revoking().is_none());
    assert_eq!(pairing.confirm_revoke(), None, "nothing was asked");

    pairing.ask_to_revoke("d1");
    assert_eq!(
        pairing.confirm_revoke(),
        Some(Command::Revoke {
            device_id: "d1".to_owned()
        })
    );
    pairing.apply(Event::Revoked {
        device_id: "d1".to_owned(),
    });
    assert!(pairing.devices()[0].revoked);
    pairing.ask_to_revoke("d1");
    assert!(
        pairing.revoking().is_none(),
        "a revoked device is not asked about again"
    );
}

#[test]
fn a_revocation_that_failed_says_so_with_the_engines_words() {
    let mut pairing = adopted(true);
    pairing.apply(Event::RevokeFailed {
        device_id: "d1".to_owned(),
        refusal: refusal(ErrorKind::ForbiddenRole, "Only the app revokes."),
    });
    assert!(
        pairing
            .revoke_failure()
            .unwrap()
            .ends_with("Only the app revokes.")
    );
    pairing.ask_to_revoke("d1");
    assert_eq!(pairing.revoke_failure(), None, "asking again clears it");
}

#[test]
fn the_snapshot_lists_the_devices_and_the_requests_that_wait() {
    let mut pairing = Pairing::default();
    let requests = [
        json!({"kind": "device_pair", "requestId": "r1", "deviceName": "Phone",
            "fingerprint": FINGERPRINT, "expiresAt": "2030-01-01T00:00:00Z"}),
        json!({"kind": "permission", "requestId": "p1"}),
    ];
    pairing.adopt(
        &[json!("pairing-code")],
        vec![device("d1")],
        &requests,
        true,
    );
    assert!(pairing.offers_code());
    assert_eq!(pairing.devices().len(), 1);
    assert_eq!(pairing.requests().len(), 1);
    assert_eq!(pairing.requests()[0].request_id, "r1");
}

#[test]
fn nothing_prints_a_typed_code_or_the_link_of_a_window() {
    let mut pairing = adopted(true);
    requested(&mut pairing, "r1", FINGERPRINT);
    let command = pairing.decide("r1", true, "482913").unwrap();
    assert!(!format!("{command:?}").contains("482913"));
    let shown = format!("{:?}", offer(5.0));
    assert!(!shown.contains("soyeht-next") && !shown.contains("engineId"));
}

#[test]
fn an_offer_reads_the_engines_answer_and_names_why_a_phone_cannot_reach_the_computer() {
    let started: DevicePairStartResult = serde_json::from_value(json!({
        "code": "c", "expiresAt": "2030-01-01T00:10:00Z",
        "uri": "soyeht-next://pair?x=1", "qr": "SOYEHT-PAIR 1 x=1",
        "reachability": {"remote": false, "reason": "loopback_only"}
    }))
    .unwrap();
    let offer = Offer::of(started, 0.0);
    assert!(offer.matrix.is_some());
    assert_eq!(offer.link, "soyeht-next://pair?x=1");
    assert_eq!(offer.expires_at, 1_893_456_600.0);
    assert_eq!(
        offer.problem.as_deref(),
        Some("Not reachable from your devices: this computer listens on loopback only")
    );
    // An end the engine wrote unreadably is the contract's length from now.
    let started: DevicePairStartResult = serde_json::from_value(
        json!({"code": "c", "expiresAt": "soon", "uri": "soyeht-next://pair?x=1"}),
    )
    .unwrap();
    let offer = Offer::of(started, 50.0);
    assert_eq!(
        offer.expires_at,
        50.0 + limits::PAIRING_WINDOW_SECONDS as f64
    );
    assert_eq!(offer.problem, None);
}
