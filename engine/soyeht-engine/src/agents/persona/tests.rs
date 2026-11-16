use super::*;
use crate::ports::testing::MemoryStore;
use soyeht_acp::session::Initialization;
use soyeht_protocol::PromptOrigin;

fn persona(text: &str) -> Persona {
    Persona {
        name: "Sabio".into(),
        text: text.into(),
    }
}

/// An avatar pane's state whose adapter did or did not advertise a system prompt.
fn state(system_prompt: bool) -> State {
    let mut state = crate::test_panes::ready_agent_state();
    state.avatar = true;
    let capabilities = if system_prompt {
        json!({"systemPrompt": {"mode": {"override": {}}}})
    } else {
        json!({"additionalDirectories": {}})
    };
    state.initialized = Some(
        Initialization::parse(&json!({"protocolVersion": 1,
            "agentCapabilities": {"sessionCapabilities": capabilities}}))
        .unwrap(),
    );
    state
}

#[test]
fn the_persona_is_who_the_avatar_is_then_the_persons_own_text() {
    assert_eq!(
        compose(&persona("Speak in one sentence.\n")),
        "You are Sabio, the avatar agent of this project.\n\nSpeak in one sentence."
    );
    // Nothing is added to a persona the person left empty.
    assert_eq!(
        compose(&persona("  \n")),
        "You are Sabio, the avatar agent of this project."
    );
}

#[test]
fn only_an_avatar_pane_has_a_persona() {
    let mut other = state(true);
    other.avatar = false;
    assert_eq!(
        text(&MemoryStore::default(), &other, "workspace").unwrap(),
        None
    );
    // An avatar pane whose workspace has no avatar (any more) has none either.
    let avatar = state(true);
    assert_eq!(
        text(&MemoryStore::default(), &avatar, "workspace").unwrap(),
        None
    );
}

#[test]
fn an_adapter_that_advertises_a_system_prompt_gets_the_persona_appended_to_it() {
    let session = system_prompt(&state(true), Some("Persona text"), json!({"cwd": "/work"}));
    assert_eq!(
        session["systemPrompt"],
        json!({"mode": "append", "content": [{"type": "text", "text": "Persona text"}]})
    );
    assert_eq!(session["cwd"], "/work");
}

#[test]
fn an_adapter_that_does_not_advertise_one_is_never_sent_the_field() {
    // It would ignore the field without an error, and the persona would be lost.
    let session = system_prompt(&state(false), Some("Persona text"), json!({"cwd": "/work"}));
    assert!(session.get("systemPrompt").is_none());
    let none = system_prompt(&state(true), None, json!({"cwd": "/work"}));
    assert!(none.get("systemPrompt").is_none());
}

#[test]
fn the_first_prompt_carries_the_persona_only_where_there_is_no_system_prompt() {
    let mut taken = state(true);
    queue_first_prompt(&mut taken, Some("Persona text"), 1, "t".into()).unwrap();
    assert!(taken.queue.is_empty());

    let mut prompted = state(false);
    queue_first_prompt(&mut prompted, Some("Persona text"), 1, "t".into()).unwrap();
    // A second open of the same session does not queue it again.
    queue_first_prompt(&mut prompted, Some("Persona text"), 1, "t".into()).unwrap();
    assert_eq!(prompted.queue.len(), 1);
    let queued = &prompted.queue[0];
    assert_eq!(queued.origin, PromptOrigin::Automatic);
    assert!(queued.text.starts_with("Persona text\n\n"));
    assert!(queued.text.ends_with(PROMPT_NOTE));

    let mut empty = state(false);
    queue_first_prompt(&mut empty, None, 1, "t".into()).unwrap();
    assert!(empty.queue.is_empty());
}

#[test]
fn a_new_session_of_a_new_generation_is_told_again() {
    let mut prompted = state(false);
    queue_first_prompt(&mut prompted, Some("Persona text"), 1, "t".into()).unwrap();
    prompted.queue.clear();
    queue_first_prompt(&mut prompted, Some("Persona text"), 2, "t".into()).unwrap();
    assert_eq!(prompted.queue.len(), 1);
}
