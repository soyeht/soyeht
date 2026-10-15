//! The cases of `contracts/vectors/agent-cards.json`, which the Mac client passes as well.

use super::*;
use crate::{testing::vector, timestamp::seconds_since_epoch};
use serde_json::Value;
use soyeht_protocol::{
    AgentSession, AuthMethod, PaneState, PromptItem, SubagentRun, TranscriptMessage,
};
use std::time::Duration;

fn cases(key: &str) -> Vec<Value> {
    vector("agent-cards")[key].as_array().unwrap().clone()
}

fn labels(key: &str) -> Value {
    vector("agent-cards")[key].clone()
}

fn text(case: &Value, key: &str) -> Option<String> {
    case[key].as_str().map(str::to_owned)
}

fn many<T: serde::de::DeserializeOwned>(value: &Value) -> Vec<T> {
    serde_json::from_value(value.clone()).unwrap()
}

#[test]
fn elapsed_time_reads_as_minutes_and_seconds_then_hours_and_minutes() {
    for case in cases("elapsedText") {
        let seconds = case["seconds"].as_u64().unwrap();
        assert_eq!(
            crate::agent::elapsed_text(Duration::from_secs(seconds)),
            case["text"].as_str().unwrap(),
            "{seconds}"
        );
    }
}

#[test]
fn the_silence_notice_says_what_the_mac_says() {
    for case in cases("silence") {
        let name = case["name"].as_str().unwrap();
        let state: Option<PaneState> = serde_json::from_value(case["paneState"].clone()).unwrap();
        let now = seconds_since_epoch(case["now"].as_str().unwrap()).unwrap();
        let silence =
            SilencePresentation::new(case["silentSince"].as_str().unwrap(), state.as_ref(), now);
        assert_eq!(
            silence.elapsed_seconds,
            case["elapsedSeconds"].as_u64(),
            "{name}"
        );
        assert_eq!(
            silence.escalated,
            case["escalated"].as_bool().unwrap(),
            "{name}"
        );
        assert_eq!(silence.elapsed(), text(&case, "elapsed"), "{name}");
        assert_eq!(Some(silence.title()), text(&case, "title"), "{name}");
        assert_eq!(
            Some(silence.detail().to_owned()),
            text(&case, "detail"),
            "{name}"
        );
    }
    assert_eq!(
        labels("silenceLabels")["cancelTurn"],
        super::silence::CANCEL_TURN
    );
}

#[test]
fn the_queue_counts_and_describes_what_waits() {
    for case in cases("queue") {
        let name = case["name"].as_str().unwrap();
        let items: Vec<PromptItem> = many(&case["items"]);
        let paused = case["paused"].as_bool().unwrap();
        let queue = QueuePresentation::new(&items, paused);
        assert_eq!(
            Some(queue.summary.clone()),
            text(&case, "summary"),
            "{name}"
        );
        assert!(QueuePresentation::shown(&items, paused), "{name}");
        let rows: Vec<Value> = queue
            .rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "sender": row.sender, "text": row.text, "attachmentCount": row.attachment_count
                })
            })
            .collect();
        assert_eq!(Value::Array(rows), case["rows"], "{name}");
    }
    assert!(!QueuePresentation::shown(&[], false));
    let words = labels("queueLabels");
    assert_eq!(words["resume"], super::queue::RESUME);
    assert_eq!(words["remove"], super::queue::REMOVE);
    assert_eq!(words["sendNow"], super::queue::SEND_NOW);
    assert_eq!(words["sendNowHint"], super::queue::SEND_NOW_HINT);
}

fn session(login: &Value) -> AgentSession {
    serde_json::from_value(serde_json::json!({
        "harness": "claude", "version": "1", "fingerprint": "f", "options": [],
        "authMethods": [], "queue": [], "draft": false, "role": "worker", "login": login
    }))
    .unwrap()
}

#[test]
fn the_sign_in_card_follows_the_engines_state() {
    for case in cases("signIn") {
        let name = case["name"].as_str().unwrap();
        let methods: Vec<AuthMethod> = many(&case["authMethods"]);
        let agent = session(&case["login"]);
        let failure = case["failure"].as_str();
        let state = SignInState::new(Some(&agent), &methods, failure);
        let kind = match &state {
            SignInState::Hidden => "hidden",
            SignInState::Choose { .. } => "choose",
            SignInState::Waiting { .. } => "waiting",
            SignInState::Verifying => "verifying",
            SignInState::SigningOut => "signingOut",
            SignInState::Failed { .. } => "failed",
        };
        assert_eq!(kind, case["state"].as_str().unwrap(), "{name}");
        assert_eq!(
            state.title().map(str::to_owned),
            text(&case, "title"),
            "{name}"
        );
        assert_eq!(
            state.detail().map(str::to_owned),
            text(&case, "detail"),
            "{name}"
        );
        match state {
            SignInState::Waiting { terminal_pane_id } => {
                assert_eq!(terminal_pane_id, text(&case, "terminalPaneId"), "{name}");
            }
            SignInState::Choose { methods, failure } => {
                let listed: Vec<Value> = methods
                    .iter()
                    .map(|method| {
                        serde_json::json!({
                            "id": method.id, "name": method.name, "detail": method.detail,
                            "opensTerminal": method.opens_terminal
                        })
                    })
                    .collect();
                assert_eq!(Value::Array(listed), case["methods"], "{name}");
                assert_eq!(failure, text(&case, "chooseFailure"), "{name}");
            }
            SignInState::Failed { reason } => assert_eq!(Some(reason), text(&case, "reason")),
            _ => {}
        }
    }
    let words = labels("signInLabels");
    assert_eq!(
        words["chooseTerminalHint"],
        super::sign_in::CHOOSE_TERMINAL_HINT
    );
    assert_eq!(words["showTerminal"], super::sign_in::SHOW_TERMINAL);
    assert_eq!(words["cancelSignIn"], super::sign_in::CANCEL_SIGN_IN);
    assert_eq!(words["dismiss"], super::sign_in::DISMISS);
}

#[test]
fn a_question_is_a_form_of_the_schemas_properties() {
    for case in cases("elicitation") {
        let listed: Vec<Value> = fields(&case["schema"])
            .iter()
            .map(|field| serde_json::json!({"key": field.key, "title": field.title}))
            .collect();
        assert_eq!(Value::Array(listed), case["fields"], "{}", case["name"]);
    }
    let words = labels("elicitationLabels");
    assert_eq!(words["cancel"], super::elicitation::CANCEL);
    assert_eq!(words["submit"], super::elicitation::SUBMIT);
}

fn shape(entries: &[Entry<'_>]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| match entry {
            Entry::Message(message) => message.text.clone(),
            Entry::Subagent(group) => format!(
                "{}[{}]",
                group.run.map_or("?", |run| run.name.as_str()),
                shape(&group.entries).join(",")
            ),
        })
        .collect()
}

#[test]
fn subagent_activity_nests_under_its_row() {
    for case in cases("subagentGrouping") {
        let name = case["name"].as_str().unwrap();
        let messages: Vec<TranscriptMessage> = many(&case["messages"]);
        let grouped = entries(&messages);
        let expected: Vec<String> = many(&case["shape"]);
        assert_eq!(shape(&grouped), expected, "{name}");
        assert_eq!(
            awaited(&grouped) as u64,
            case["awaited"].as_u64().unwrap(),
            "{name}"
        );
    }
    for case in cases("subagentGroupingEntryIds") {
        let messages: Vec<TranscriptMessage> = many(&case["messages"]);
        let ids: Vec<String> = entries(&messages).iter().map(Entry::id).collect();
        assert_eq!(ids, many::<String>(&case["ids"]), "{}", case["name"]);
    }
}

#[test]
fn a_subagent_that_names_itself_does_not_nest_for_ever() {
    let looped: Vec<TranscriptMessage> = many(&serde_json::json!([{
        "localId": "m1", "generation": 1, "seq": "1", "role": "assistant", "kind": "subagent",
        "text": "", "complete": true, "subagentId": "a",
        "subagent": {"subagentId": "a", "name": "A", "task": "t", "status": "running"}
    }]));
    assert!(entries(&looped).is_empty());
}

#[test]
fn a_subagent_row_says_how_it_goes() {
    for case in cases("subagentPresentation") {
        let name = case["name"].as_str().unwrap();
        let run: Option<SubagentRun> = serde_json::from_value(case["run"].clone()).unwrap();
        let now = seconds_since_epoch(case["now"].as_str().unwrap()).unwrap();
        let row = SubagentPresentation::new(run.as_ref(), now);
        let status = match row.status {
            Status::Running => "running",
            Status::Succeeded => "succeeded",
            Status::Failed => "failed",
            Status::Stopped => "stopped",
        };
        assert_eq!(status, case["status"].as_str().unwrap(), "{name}");
        assert_eq!(Some(row.trailing), text(&case, "trailing"), "{name}");
        assert_eq!(row.model, text(&case, "model"), "{name}");
        assert_eq!(row.summary, text(&case, "summary"), "{name}");
    }
    let unnamed = SubagentPresentation::new(None, 0.0);
    assert_eq!(
        Some(unnamed.name),
        text(&vector("agent-cards"), "subagentRunFallbackName")
    );
}

#[test]
fn the_parent_says_how_many_subagents_it_waits_on() {
    for case in cases("subagentWaiting") {
        let count = case["count"].as_u64().unwrap() as usize;
        assert_eq!(Some(waiting(count)), text(&case, "text"));
    }
}

#[test]
fn the_preview_keeps_the_meaningful_lines_and_cuts_at_a_word() {
    for case in cases("subagentPreview") {
        let limit = case["limit"].as_u64().unwrap() as usize;
        assert_eq!(
            preview(case["text"].as_str().unwrap(), limit),
            text(&case, "preview"),
            "{}",
            case["text"]
        );
    }
}

#[test]
fn the_button_for_earlier_messages_is_named_as_on_the_mac() {
    assert_eq!(labels("transcriptLabels")["loadEarlier"], LOAD_EARLIER);
}
