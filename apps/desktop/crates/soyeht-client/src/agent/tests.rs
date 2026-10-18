use super::*;
use crate::testing::{cases, pane_json};
use serde_json::{Value, json};

fn message_json(id: &str, seq: &str, kind: &str, text: &str) -> Value {
    json!({"localId": id, "generation": 1, "seq": seq, "role": "assistant", "kind": kind,
           "text": text, "complete": true})
}

fn message(id: &str, seq: &str, text: &str) -> TranscriptMessage {
    serde_json::from_value(message_json(id, seq, "message", text)).unwrap()
}

fn tool(seq: &str, title: &str, status: &str) -> TranscriptMessage {
    let mut value = message_json(seq, seq, "tool", "");
    value["tool"] = json!({"toolCallId": seq, "title": title, "status": status,
                           "locations": []});
    serde_json::from_value(value).unwrap()
}

#[test]
fn merging_replaces_by_local_id_and_orders_by_sequence() {
    let mut view = AgentView::default();
    view.merge(message("b", "2", "second")).unwrap();
    view.merge(message("a", "1", "first")).unwrap();
    view.merge(message("b", "2", "second, grown")).unwrap();
    let texts: Vec<_> = view.messages.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, ["first", "second, grown"]);
}

fn partial(id: &str, seq: &str, text: &str, complete: bool) -> TranscriptMessage {
    let mut value = message(id, seq, text);
    value.complete = complete;
    value
}

#[test]
fn a_page_fetched_earlier_never_undoes_a_newer_update() {
    let mut view = AgentView::default();
    view.merge(partial("m", "2", "Hello, wor", false)).unwrap();
    view.merge(partial("m", "2", "Hello, world", true)).unwrap();
    // The page was read before the last chunks: it is behind what is held.
    view.merge_page(vec![
        partial("m", "2", "Hello", false),
        message("a", "1", "first"),
    ])
    .unwrap();
    let held: Vec<_> = view
        .messages
        .iter()
        .map(|m| (m.text.as_str(), m.complete))
        .collect();
    assert_eq!(held, [("first", true), ("Hello, world", true)]);
}

#[test]
fn a_page_adds_messages_without_dropping_the_known_ones() {
    let mut view = AgentView::default();
    view.merge(message("live", "9", "newer")).unwrap();
    view.merge_page(vec![message("old", "1", "older")]).unwrap();
    assert_eq!(view.messages.len(), 2);
    assert_eq!(view.messages[0].local_id, "old");
}

#[test]
fn a_sequence_that_is_not_a_decimal_is_refused() {
    let mut view = AgentView::default();
    assert!(view.merge(message("x", "not a number", "text")).is_err());
    assert!(view.messages.is_empty());
}

#[test]
fn the_tail_leaves_out_the_oldest_messages() {
    let mut view = AgentView::default();
    for index in 0..(TAIL + 5) {
        view.merge(message(&format!("m{index}"), &index.to_string(), "t"))
            .unwrap();
    }
    let (shown, hidden) = view.tail(TAIL);
    assert_eq!((shown.len(), hidden), (TAIL, 5));
    assert_eq!(shown[0].local_id, "m5");
}

#[test]
fn a_message_update_becomes_a_transcript_message() {
    let update: PaneUpdateEvent = serde_json::from_value(json!({
        "eventId": "9", "at": "now", "paneId": "p", "kind": "agent_message_chunk",
        "generation": 1, "seq": "3", "localId": "m", "text": "Hello", "complete": false
    }))
    .unwrap();
    let (pane_id, message) = message_of(update).unwrap();
    assert_eq!(pane_id, "p");
    assert_eq!(message.role, TranscriptRole::Assistant);
    assert_eq!(message.kind, TranscriptKind::Message);
    assert!(!message.complete);
}

#[test]
fn a_tool_update_carries_the_call_and_completes_with_it() {
    let update: PaneUpdateEvent = serde_json::from_value(json!({
        "eventId": "9", "at": "now", "paneId": "p", "kind": "tool_call",
        "generation": 1, "seq": "4", "localId": "t",
        "tool": {"toolCallId": "c", "title": "ls", "kind": "execute",
                 "status": "completed", "locations": []}
    }))
    .unwrap();
    let (_, message) = message_of(update).unwrap();
    assert_eq!(message.kind, TranscriptKind::Tool);
    assert!(message.complete);
    let mut view = AgentView::default();
    view.merge(message).unwrap();
    assert_eq!(view.ticker(None).as_deref(), Some("Done: ls"));
}

#[test]
fn a_commands_output_is_its_bytes_read_as_text() {
    let output = |bytes: &str| -> TerminalOutput {
        serde_json::from_value(json!({
            "terminalId": "t", "bytes": bytes, "offset": "0", "truncated": false
        }))
        .unwrap()
    };
    assert_eq!(terminal_text(&output("aGkKb2s=")), "hi\nok");
    assert_eq!(terminal_text(&output("/w==")), "\u{fffd}");
    assert_eq!(terminal_text(&output("not base64!")), "");
}

#[test]
fn the_ticker_lists_the_current_step_then_earlier_tool_calls_then_the_title() {
    let mut view = AgentView::default();
    assert_eq!(view.ticker(None), None);
    assert_eq!(view.ticker(Some("  ")), None);
    for (seq, title, status) in [
        ("1", "read a", "completed"),
        ("2", "edit b", "completed"),
        ("3", "test c", "in_progress"),
    ] {
        view.merge(tool(seq, title, status)).unwrap();
    }
    let mut plan = message_json("p", "4", "plan", "");
    plan["plan"] = json!([
        {"content": "write", "priority": "high", "status": "completed"},
        {"content": "check", "priority": "high", "status": "in_progress"}
    ]);
    view.merge(serde_json::from_value(plan).unwrap()).unwrap();
    let line = view.ticker(Some("The title")).unwrap();
    assert_eq!(
        line,
        [
            "Step 2/2: check \u{b7} Running test c",
            "Done: edit b",
            "Done: read a",
            "The title"
        ]
        .join(TICKER_SEPARATOR)
    );
}

#[test]
fn a_failed_tool_call_leaves_the_ticker_when_the_next_turn_starts_or_a_retry_succeeds() {
    let prompt = |seq: &str| {
        let mut value = message_json(seq, seq, "message", "go");
        value["role"] = json!("user");
        serde_json::from_value::<TranscriptMessage>(value).unwrap()
    };
    let mut view = AgentView::default();
    for message in [prompt("1"), tool("2", "Write out", "failed")] {
        view.merge(message).unwrap();
    }
    assert_eq!(view.ticker(None).as_deref(), Some("Failed: Write out"));
    view.merge(tool("3", "Write out", "completed")).unwrap();
    assert_eq!(view.ticker(None).as_deref(), Some("Done: Write out"));
    let mut next = AgentView::default();
    for message in [prompt("1"), tool("2", "Write out", "failed"), prompt("3")] {
        next.merge(message).unwrap();
    }
    assert_eq!(next.ticker(None), None);
}

#[test]
fn a_turn_is_timed_from_the_moment_its_prompt_appears() {
    let start = Instant::now();
    let mut clock = TurnClock::default();
    assert_eq!(clock.elapsed(start), None);
    clock.observe(Some("p1"), start);
    clock.observe(Some("p1"), start + Duration::from_secs(5));
    assert_eq!(
        clock.elapsed(start + Duration::from_secs(65)),
        Some(Duration::from_secs(65))
    );
    clock.observe(Some("p2"), start + Duration::from_secs(70));
    assert_eq!(
        clock.elapsed(start + Duration::from_secs(71)),
        Some(Duration::from_secs(1))
    );
    clock.observe(None, start + Duration::from_secs(80));
    assert_eq!(clock.elapsed(start + Duration::from_secs(90)), None);
    assert_eq!(elapsed_text(Duration::from_secs(125)), "2m 5s");
    assert_eq!(elapsed_text(Duration::from_secs(3_780)), "1h 3m");
}

#[test]
fn the_context_ring_follows_the_last_usage_report() {
    let mut view = AgentView::default();
    assert_eq!(view.context_fraction(), None);
    let mut usage = message("u", "5", "");
    usage.kind = TranscriptKind::Usage;
    usage.usage =
        serde_json::from_value(json!({"totalTokens": "250", "contextSize": "1000"})).unwrap();
    view.merge(usage).unwrap();
    assert_eq!(view.context_fraction(), Some(0.25));
}

fn agent_pane(state: &str) -> Pane {
    let mut value = pane_json("p", "w", "agent");
    value["state"] = json!(state);
    value["agent"] = json!({
        "harness": "claude", "version": "1", "fingerprint": "f", "authMethods": [],
        "queue": [], "draft": false, "role": "worker",
        "options": [
            {"id": "model", "name": "Model", "type": "select", "currentValue": "opus",
             "options": [{"value": "opus", "name": "Opus"}, {"value": "sonnet", "name": "Sonnet"}]},
            {"id": "mode", "name": "Mode", "type": "select", "currentValue": "manual",
             "options": [{"value": "manual", "name": "Manual"}]}
        ]
    });
    serde_json::from_value(value).unwrap()
}

#[test]
fn the_control_row_comes_from_the_options_the_harness_advertises() {
    let pane = agent_pane("idle");
    let controls = Controls::new(&pane, None, Some("Claude"), false, true).unwrap();
    assert_eq!(controls.model.as_ref().map(Menu::label), Some("Opus"));
    assert_eq!(
        controls.permission.as_ref().map(Menu::label),
        Some("Manual")
    );
    assert!(controls.effort.is_none());
    assert_eq!(controls.placeholder, "Message Claude\u{2026}");
    assert_eq!(controls.action, Action::Send { enabled: false });
    assert_eq!(controls.context_label(), "\u{2013}");
}

#[test]
fn a_running_agent_turns_send_into_stop_and_queues_messages() {
    let pane = agent_pane("working");
    let controls = Controls::new(&pane, None, None, true, true).unwrap();
    assert_eq!(controls.action, Action::Stop { enabled: true });
    assert_eq!(controls.placeholder, "Queue a message for claude\u{2026}");
    let cancelling = agent_pane("cancelling");
    let controls = Controls::new(&cancelling, None, None, true, true).unwrap();
    assert_eq!(controls.action, Action::Stop { enabled: false });
}

#[test]
fn a_pane_without_a_session_has_no_control_row() {
    let pane: Pane = serde_json::from_value(pane_json("p", "w", "agent")).unwrap();
    assert!(Controls::new(&pane, None, None, true, true).is_none());
}

#[test]
fn the_status_word_names_the_state() {
    assert_eq!(status_word(&PaneState::Idle, false, true), "idle");
    assert_eq!(
        status_word(&PaneState::WaitingForYou, false, true),
        "waiting for you"
    );
    assert_eq!(status_word(&PaneState::Starting, false, true), "ready");
    assert_eq!(status_word(&PaneState::Idle, true, true), "Sign in");
}

fn state(name: &str) -> PaneState {
    serde_json::from_value(Value::from(name)).unwrap()
}

#[test]
fn the_status_word_is_the_one_in_the_vector() {
    for case in cases("pane-state-presentation", "statusWords") {
        assert_eq!(
            status_word(
                &state(case["state"].as_str().unwrap()),
                case["needsAuthentication"].as_bool().unwrap(),
                case["hasSession"].as_bool().unwrap(),
            ),
            case["title"].as_str().unwrap(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn every_state_word_is_its_wire_spelling_spaced() {
    for case in cases("pane-state-presentation", "states") {
        let wire = case["state"].as_str().unwrap();
        assert_eq!(
            status_word(&state(wire), false, false),
            case["word"].as_str().unwrap()
        );
    }
}
