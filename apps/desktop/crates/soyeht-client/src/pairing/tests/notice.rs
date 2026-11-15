//! What a person is told of devices added, the panes a device made before it is revoked and a
//! tailnet change that waits for them (ADR 0164).

use super::*;

#[test]
fn only_an_engine_that_asks_for_the_code_is_asked_for_a_notice_and_one_that_does_not_has_none() {
    assert_eq!(adopted(true).load_notice(), Some(Command::LoadNotice));
    let mut mac = adopted(false);
    assert_eq!(mac.load_notice(), None);
    // A device with no date, as every device of a Mac is, is not news.
    mac.notified(
        &EventName::DevicePaired,
        json!({"eventId": "1", "at": "now", "device": {"deviceId": "d", "name": "Phone",
            "fingerprint": FINGERPRINT, "revoked": false, "addedAt": "2026-11-15T10:00:00Z"}}),
    )
    .unwrap();
    assert!(mac.notice().is_empty());
}

#[test]
fn the_notice_lists_the_devices_added_and_dismissing_it_goes_through_the_newest_shown() {
    let mut pairing = adopted(true);
    pairing.apply(Event::Noticed {
        devices: vec![
            added("d1", "2026-11-15T10:00:00Z"),
            added("d2", "2026-11-15T09:00:00-03:00"),
        ],
    });
    // 09:00 at -03:00 is 12:00 UTC: the newest is the second, whatever order they came in.
    assert_eq!(
        pairing.dismiss_notice(),
        Some(Command::DismissNotice {
            through: "2026-11-15T09:00:00-03:00".to_owned()
        })
    );
    // A device added while the notice was on screen is news that dismissal did not cover.
    pairing
        .notified(
            &EventName::DevicePaired,
            json!({"eventId": "2", "at": "now", "device": {"deviceId": "d3", "name": "Phone",
                "fingerprint": FINGERPRINT, "revoked": false,
                "addedAt": "2026-11-15T13:00:00Z", "node": "n"}}),
        )
        .unwrap();
    assert_eq!(pairing.notice().len(), 3);
    pairing.apply(Event::NoticeDismissed {
        through: "2026-11-15T12:00:00Z".to_owned(),
    });
    let left: Vec<_> = pairing
        .notice()
        .iter()
        .map(|d| d.device_id.as_str())
        .collect();
    assert_eq!(left, ["d3"]);
    // A device that is revoked is no longer news.
    pairing
        .notified(
            &EventName::DeviceRevoked,
            json!({"eventId": "3", "at": "now", "deviceId": "d3"}),
        )
        .unwrap();
    assert!(pairing.notice().is_empty());
    assert_eq!(pairing.dismiss_notice(), None);
}

#[test]
fn a_device_says_when_and_from_where_it_was_added() {
    use super::copy::added_line;
    assert_eq!(
        added_line(Some("2026-11-15T10:42:09Z"), Some("phone")).as_deref(),
        Some("Added 2026-11-15 10:42 UTC from phone")
    );
    assert_eq!(
        added_line(Some("2026-11-15T07:42:09-03:00"), None).as_deref(),
        Some("Added 2026-11-15 10:42 UTC")
    );
    assert_eq!(
        added_line(None, Some("phone")).as_deref(),
        Some("Added from phone")
    );
    assert_eq!(added_line(None, None), None);
    assert_eq!(added_line(Some("yesterday"), None), None);
}

#[test]
fn the_question_before_a_revocation_asks_for_the_panes_and_closes_only_those_it_listed() {
    let mut pairing = adopted(true);
    pairing.adopt(&[json!("pairing-code")], vec![device("d1")], &[], true);
    assert_eq!(
        pairing.ask_to_revoke("d1"),
        Some(Command::LoadPanes {
            device_id: "d1".to_owned()
        })
    );
    assert_eq!(pairing.revoking_panes(), &DevicePanes::Loading);
    // The answer of another device's list is not this question's.
    pairing.apply(Event::PanesListed {
        device_id: "d2".to_owned(),
        panes: vec![pane("x")],
    });
    assert_eq!(pairing.revoking_panes(), &DevicePanes::Loading);
    pairing.apply(Event::PanesListed {
        device_id: "d1".to_owned(),
        panes: vec![pane("p1"), pane("p2")],
    });
    // Revoking and keeping them closes none; revoking and closing closes what was listed.
    let mut keep = pairing.clone();
    assert_eq!(
        keep.confirm_revoke(false),
        Some(Command::Revoke {
            device_id: "d1".to_owned(),
            close_panes: vec![]
        })
    );
    assert_eq!(
        pairing.confirm_revoke(true),
        Some(Command::Revoke {
            device_id: "d1".to_owned(),
            close_panes: vec!["p1".to_owned(), "p2".to_owned()]
        })
    );
    // A list the engine could not give leaves the person able to revoke, and closes nothing.
    pairing.ask_to_revoke("d1");
    pairing.apply(Event::PanesFailed {
        device_id: "d1".to_owned(),
        refusal: refusal(ErrorKind::DiskFull, "no"),
    });
    assert_eq!(pairing.revoking_panes(), &DevicePanes::Unknown);
    assert_eq!(
        pairing.confirm_revoke(true),
        Some(Command::Revoke {
            device_id: "d1".to_owned(),
            close_panes: vec![]
        })
    );
    // An engine that does not ask for the code does not list panes, and offers none to close.
    let mut mac = adopted(false);
    mac.adopt(&[], vec![device("d1")], &[], true);
    assert_eq!(mac.ask_to_revoke("d1"), None);
    assert_eq!(mac.revoking().map(|d| d.device_id.as_str()), Some("d1"));
    assert_eq!(mac.revoking_panes(), &DevicePanes::Listed(vec![]));
}

#[test]
fn panes_that_would_not_close_are_said_once_the_device_is_revoked() {
    let mut pairing = adopted(true);
    pairing.adopt(
        &[json!("pairing-code")],
        vec![added("d1", "2026-11-15T10:00:00Z")],
        &[],
        true,
    );
    pairing.apply(Event::Revoked {
        device_id: "d1".to_owned(),
        left_open: 2,
    });
    assert!(pairing.devices()[0].revoked);
    assert_eq!(
        pairing.revoke_failure(),
        Some("The device was revoked, but 2 of its panes could not be closed.")
    );
}

/// The window the engine answers a start with, with or without a tailnet change waiting.
fn started_with(pairing: &mut Pairing, tailnet_changed: bool) {
    let asked = pairing.open(true);
    let ticket = match asked {
        Some(Command::Start { ticket }) => ticket,
        other => panic!("a connected sheet asks for a window, not {other:?}"),
    };
    let window = Offer {
        tailnet_changed,
        ..offer(1000.0)
    };
    pairing.apply(Event::Started {
        ticket,
        offer: window,
    });
}

#[test]
fn a_tailnet_change_that_waits_is_known_from_the_window_or_from_the_engine_telling_it() {
    let mut pairing = adopted(true);
    assert!(!pairing.tailnet_changed());
    // The answer to the call that opens the window says it for a sheet opened after the refusal.
    started_with(&mut pairing, true);
    assert!(pairing.tailnet_changed());
    started_with(&mut pairing, false);
    assert!(!pairing.tailnet_changed());
    // The engine tells a sheet that is open when the refusal comes.
    let told = json!({"tailnetChanged": true});
    assert!(
        pairing
            .notified(&EventName::DevicePairTailnetChanged, told.clone())
            .unwrap()
    );
    assert!(pairing.tailnet_changed());
    // An engine that does not ask for the person's code has no such change to confirm.
    let mut mac = adopted(false);
    mac.notified(&EventName::DevicePairTailnetChanged, told)
        .unwrap();
    assert!(!mac.tailnet_changed());
    assert_eq!(mac.confirm_tailnet(true), None);
}

#[test]
fn confirming_a_tailnet_change_asks_for_a_new_window_with_the_confirmation_and_only_when_one_waits()
{
    let mut pairing = adopted(true);
    // Nothing waits: nothing is sent.
    assert_eq!(pairing.confirm_tailnet(true), None);
    started_with(&mut pairing, true);
    // No connection: the choice is kept for the next, and nothing is sent.
    assert_eq!(pairing.confirm_tailnet(false), None);
    let Some(Command::ConfirmTailnet { ticket }) = pairing.confirm_tailnet(true) else {
        panic!("the confirmation asks for a window");
    };
    assert_eq!(pairing.phase(), &Phase::Starting { ticket });
    // The window that answers has no change waiting: the notice goes, and the code shows.
    pairing.apply(Event::Started {
        ticket,
        offer: offer(1000.0),
    });
    assert!(!pairing.tailnet_changed());
    assert!(matches!(pairing.phase(), Phase::Showing(_)));
    // A refused confirmation is a refused start, and the change still waits.
    started_with(&mut pairing, true);
    let Some(command @ Command::ConfirmTailnet { ticket }) = pairing.confirm_tailnet(true) else {
        panic!("the confirmation asks for a window");
    };
    pairing.apply(command.failure(&Error::Remote(soyeht_protocol::RpcError {
        code: -32000,
        message: "no".to_owned(),
        data: ErrorData {
            kind: ErrorKind::ForbiddenRole,
            detail: None,
            retry_after_sec: None,
        },
    })));
    assert!(matches!(pairing.phase(), Phase::Refused(_)), "{ticket}");
    assert!(pairing.tailnet_changed());
}
