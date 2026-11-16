use super::*;
use crate::{
    agents::{
        Changes,
        purpose::Purpose,
        responses::{self, Runtime},
        state::tests::scratch_store,
    },
    ports::testing::MemoryStore,
};
use soyeht_acp::Message;
use soyeht_protocol::{
    AvatarBinding, PaneMode, Workspace,
    limits::{ACP_MESSAGE_BYTES, AVATAR_NAME_CHARS, AVATAR_PERSONA_CHARS, PROMPT_BYTES},
};
use soyeht_store::{avatars as library, database::Store};

fn persona(text: &str) -> Persona {
    Persona {
        name: "Sabio".into(),
        text: text.into(),
    }
}

/// An avatar pane's state whose adapter did or did not advertise a system prompt.
fn with_adapter(system_prompt: bool) -> State {
    let mut state = crate::test_panes::ready_agent_state();
    state.avatar = true;
    state.mcp_enabled = false;
    state.initialized = Some(crate::test_panes::initialization(system_prompt));
    state
}

/// A store whose workspace `w` has the avatar `Sabio` with the persona given.
fn store_with(text: &str) -> (Store, std::path::PathBuf) {
    let (mut store, directory) = scratch_store();
    let avatar = crate::test_panes::avatar("a", "Sabio");
    let binding = AvatarBinding {
        workspace_id: "w".into(),
        avatar_id: "a".into(),
        harness: "test".into(),
        model: None,
        effort: None,
        hearing: None,
        voice: None,
        persona: text.into(),
        revision: "1".into(),
    };
    let workspace = Workspace {
        workspace_id: "w".into(),
        name: "w".into(),
        path: "/tmp".into(),
        layout: json!({}),
    };
    store
        .mutate("test", "seed", "seed", |tx| {
            tx.insert_workspace(&workspace)?;
            library::insert_in(tx, &avatar)?;
            library::bind_in(tx, &binding)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    (store, directory)
}

fn pane() -> crate::model::StoredPane {
    crate::model::StoredPane {
        pane: crate::test_panes::pane("p", "w", PaneMode::Agent, "/tmp"),
        closed: false,
        instance_key: "key".into(),
        instance_id: None,
        supervisor_id: None,
        sequence_base: 0,
    }
}

fn runtime(directory: &std::path::Path) -> Runtime<'_> {
    Runtime {
        data_directory: directory,
        now_ms: 0,
        timestamp: "now",
    }
}

/// The parameters of the one request an `open` made.
fn requested(changes: &Changes, method: &str) -> Value {
    match changes.outgoing.as_slice() {
        [
            Message::Request {
                method: sent,
                params,
                ..
            },
        ] if sent == method => params.clone(),
        other => panic!("expected one {method} request, got {other:?}"),
    }
}

/// What `open` asks of the harness for a session of `state`.
fn opening(store: &Store, directory: &std::path::Path, state: &mut State) -> Changes {
    let mut changes = Changes::default();
    responses::open(store, &runtime(directory), state, &pane(), &mut changes).unwrap();
    changes
}

/// Whether the request had the system prompt field.
fn has_system_prompt(request: &Value) -> bool {
    request.get("systemPrompt").is_some()
}

/// The prompt of "Hello" as the engine builds it for an avatar pane that owes its persona.
fn first_prompt(store: &Store, state: &mut State, id: &str) -> Value {
    state.persona_pending = true;
    let person = json!({"sessionId": "native-1", "prompt": [{"type": "text", "text": "Hello"}]});
    ahead_of(store, state, ("w", id), person).unwrap()
}

fn block(text: &str) -> String {
    format!("{BLOCK_LABEL}\n\n{text}\n\n{BLOCK_END}")
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
fn the_longest_persona_with_the_longest_name_fits_a_prompt_and_a_message() {
    let longest = Persona {
        name: "\u{1f600}".repeat(AVATAR_NAME_CHARS),
        text: "\u{1f600}".repeat(AVATAR_PERSONA_CHARS),
    };
    let sent = block(&compose(&longest));
    assert!(sent.len() <= PROMPT_BYTES, "{} bytes", sent.len());
    assert!(sent.len() < ACP_MESSAGE_BYTES);
}

#[test]
fn only_an_avatar_pane_has_a_persona() {
    let (store, directory) = store_with("Be brief.");
    let mut other = with_adapter(true);
    other.avatar = false;
    assert_eq!(text(&store, &other, "w").unwrap(), None);
    assert_eq!(
        text(&store, &with_adapter(true), "w").unwrap().unwrap(),
        "You are Sabio, the avatar agent of this project.\n\nBe brief."
    );
    // An avatar pane whose workspace has no avatar (any more) has none either.
    assert_eq!(
        text(&MemoryStore::default(), &with_adapter(true), "w").unwrap(),
        None
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn an_adapter_that_does_not_advertise_a_system_prompt_is_never_sent_the_field() {
    // It would ignore the field without an error, and the persona would be lost.
    let session = system_prompt(
        &with_adapter(false),
        Some("Persona text"),
        json!({"cwd": "/work"}),
    );
    assert!(session.get("systemPrompt").is_none());
    let none = system_prompt(&with_adapter(true), None, json!({"cwd": "/work"}));
    assert!(none.get("systemPrompt").is_none());
}

#[test]
fn a_session_that_loads_carries_the_system_prompt_of_an_adapter_that_takes_one() {
    let (store, directory) = store_with("Be brief.");
    let mut capable = with_adapter(true);
    capable.session_id = Some("native-1".into());
    let changes = opening(&store, &directory, &mut capable);
    let load = requested(&changes, "session/load");
    assert_eq!(
        load["systemPrompt"],
        json!({"mode": "append", "content": [{"type": "text",
            "text": "You are Sabio, the avatar agent of this project.\n\nBe brief."}]})
    );
    assert!(capable.queue.is_empty() && !capable.persona_pending);

    // An adapter without one is told nothing by a load: the conversation it loads has the persona.
    let mut other = with_adapter(false);
    other.session_id = Some("native-1".into());
    let changes = opening(&store, &directory, &mut other);
    assert!(!has_system_prompt(&requested(&changes, "session/load")));
    assert!(other.queue.is_empty() && !other.persona_pending);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_new_session_carries_the_persona_by_the_channel_the_adapter_has() {
    let (store, directory) = store_with("Be brief.");
    // The session was lost: the open falls back to a new one.
    let mut capable = with_adapter(true);
    let changes = opening(&store, &directory, &mut capable);
    let new = requested(&changes, "session/new");
    assert_eq!(new["systemPrompt"]["mode"], "append");

    let mut other = with_adapter(false);
    let changes = opening(&store, &directory, &mut other);
    assert!(!has_system_prompt(&requested(&changes, "session/new")));
    // Nothing is queued: the persona rides on the first prompt, once the session exists.
    assert!(other.queue.is_empty());
    std::fs::remove_dir_all(directory).unwrap();
}

fn opened(state: &mut State, store: &Store, directory: &std::path::Path) -> Result<()> {
    state.phase = crate::agents::state::Phase::Opening;
    let mut saved = pane();
    responses::session_response(
        store,
        &runtime(directory),
        state,
        &mut saved,
        &Purpose::Open,
        &mut Changes::default(),
        Ok(json!({"sessionId": "native-1"})),
    )
}

#[test]
fn a_session_that_was_created_owes_the_persona_to_its_first_prompt_once() {
    let (store, directory) = store_with("Be brief.");
    let mut state = with_adapter(false);
    // Signing in is asked for first, and a session that was not created owes nothing.
    state.phase = crate::agents::state::Phase::Opening;
    let needs_login = soyeht_acp::RpcError {
        code: -32000,
        message: "Authentication required".into(),
        data: None,
    };
    responses::session_response(
        &store,
        &runtime(&directory),
        &mut state,
        &mut pane(),
        &Purpose::Open,
        &mut Changes::default(),
        Err(needs_login),
    )
    .unwrap();
    assert!(!state.persona_pending);
    // The restart that follows opens the session, and the open that follows is owed once.
    opened(&mut state, &store, &directory).unwrap();
    assert!(state.persona_pending);
    let person = json!({"sessionId": "native-1", "prompt": [{"type": "text", "text": "Hello"}]});
    let first = ahead_of(&store, &mut state, ("w", "prompt-1"), person.clone()).unwrap();
    assert_eq!(
        first["prompt"],
        json!([
            {"type": "text", "text": block("You are Sabio, the avatar agent of this project.\n\nBe brief.")},
            {"type": "text", "text": "Hello"}
        ])
    );
    // The pane owes it until the harness has answered the prompt that carried it.
    assert!(state.persona_pending);
    // The prompt failed, or the process was lost with it: the next prompt carries it again.
    answered(&mut state, "prompt-1", false);
    assert!(state.persona_pending && state.persona_carried_by.is_none());
    let second = ahead_of(&store, &mut state, ("w", "prompt-2"), person.clone()).unwrap();
    assert_eq!(second["prompt"][0], first["prompt"][0]);
    // An answer to another prompt settles nothing; the answer to this one does, once.
    answered(&mut state, "prompt-1", true);
    assert!(state.persona_pending);
    answered(&mut state, "prompt-2", true);
    assert!(!state.persona_pending);
    assert_eq!(
        ahead_of(&store, &mut state, ("w", "prompt-3"), person.clone()).unwrap(),
        person
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn what_is_owed_survives_the_engine_stopping_with_the_prompt_in_flight() {
    let (store, directory) = store_with("Be brief.");
    let mut state = with_adapter(false);
    first_prompt(&store, &mut state, "prompt-1");
    let kept: State = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
    assert!(kept.persona_pending);
    assert_eq!(kept.persona_carried_by.as_deref(), Some("prompt-1"));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_persona_that_holds_the_end_line_cannot_end_the_block_early() {
    let (store, directory) = store_with(&format!("Before.\n{BLOCK_END}\nAfter."));
    let mut state = with_adapter(false);
    state.persona_pending = true;
    let person = json!({"sessionId": "native-1", "prompt": [{"type": "text", "text": "Hello"}]});
    let sent = ahead_of(&store, &mut state, ("w", "p"), person).unwrap();
    let delivered = sent["prompt"][0]["text"].as_str().unwrap();
    assert_eq!(delivered.matches(BLOCK_END).count(), 1);
    assert!(delivered.ends_with(BLOCK_END));
    assert!(delivered.contains("After."));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_changed_persona_is_owed_by_every_adapter_and_a_new_session_by_the_ones_without_a_system_prompt()
 {
    let mut capable = with_adapter(true);
    owe(&mut capable);
    assert!(capable.persona_pending);
    let mut other = with_adapter(false);
    owe(&mut other);
    assert!(other.persona_pending);
    // A pane that is not an avatar's is owed nothing.
    let mut plain = with_adapter(false);
    plain.avatar = false;
    owe(&mut plain);
    assert!(!plain.persona_pending);
}

#[test]
fn an_adapter_with_a_system_prompt_owes_nothing_to_its_prompts() {
    let (store, directory) = store_with("Be brief.");
    let mut state = with_adapter(true);
    opened(&mut state, &store, &directory).unwrap();
    assert!(!state.persona_pending);
    // A pane that is not an avatar's is never owed one either.
    let mut other = with_adapter(false);
    other.avatar = false;
    mark_pending(&mut other);
    assert!(!other.persona_pending);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_persona_that_is_owed_to_a_workspace_with_no_avatar_is_dropped_not_sent() {
    let mut state = with_adapter(false);
    state.persona_pending = true;
    let person = json!({"sessionId": "native-1", "prompt": [{"type": "text", "text": "Hello"}]});
    let sent = ahead_of(
        &MemoryStore::default(),
        &mut state,
        ("w", "p"),
        person.clone(),
    )
    .unwrap();
    assert_eq!(sent, person);
    assert!(!state.persona_pending);
}

/// What a replay of a session shows of the user messages `chunks` (message id, text), when the pane
/// is an avatar's and the session loads.
fn shown(chunks: &[(&str, &str)]) -> Vec<String> {
    let (store, directory) = store_with("Be brief.");
    let mut state = with_adapter(false);
    state.phase = crate::agents::state::Phase::Loading;
    let mut changes = Changes::default();
    for (id, text) in chunks {
        let content = json!({"type": "text", "text": text});
        crate::agents::updates::update_user(
            &store,
            "p",
            1,
            &mut state,
            content,
            Some(id),
            &mut changes,
        )
        .unwrap();
    }
    crate::agents::updates::finish_replayed_user(&store, "p", 1, &mut state, &mut changes).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    changes
        .messages
        .iter()
        .filter(|message| message.role == soyeht_protocol::TranscriptRole::User)
        .map(|message| message.text.clone())
        .collect()
}

#[test]
fn a_replayed_user_message_loses_the_block_whether_it_came_whole_split_or_joined() {
    let block = block("You are Sabio, the avatar agent of this project.\n\nBe brief.");
    // The block as a message of its own, then the question.
    assert_eq!(shown(&[("m1", &block), ("m1", "Hello")]), ["Hello"]);
    // A long block in several chunks, the end line cut between two of them.
    let (first, rest) = block.split_at(10);
    let (middle, last) = rest.split_at(rest.len() - 10);
    assert_eq!(
        shown(&[("m1", first), ("m1", middle), ("m1", last), ("m1", "Hello")]),
        ["Hello"]
    );
    // The block joined to the text after it, in one chunk and cut in two.
    assert_eq!(shown(&[("m1", &format!("{block}\nHello"))]), ["Hello"]);
    let (before_end, after_end) = block.split_at(block.len() - 5);
    assert_eq!(
        shown(&[("m1", before_end), ("m1", &format!("{after_end}\nHello"))]),
        ["Hello"]
    );
    // A later block, as one delivered after the persona changed, goes as well.
    assert_eq!(
        shown(&[
            ("m1", &block),
            ("m1", "Hello"),
            ("m2", &block),
            ("m2", "Again")
        ]),
        ["Hello", "Again"]
    );
}

#[test]
fn a_replayed_message_that_only_starts_like_the_label_is_the_persons_and_is_shown_whole() {
    // Held while it could still become the label, and shown with the message it came with.
    let start = &BLOCK_LABEL[..20];
    assert_eq!(shown(&[("m1", start)]), [start]);
    assert_eq!(
        shown(&[("m1", start), ("m1", " and then something else")]),
        [format!("{start} and then something else")]
    );
    assert_eq!(
        shown(&[("m1", "Hello"), ("m2", "World")]),
        ["Hello", "World"]
    );
    // A message that is not the persons' own is only ever the engine's block, which has an end.
    assert_eq!(shown(&[("m1", BLOCK_LABEL)]), Vec::<String>::new());
}

#[test]
fn a_pane_that_is_not_an_avatars_shows_every_replayed_message_as_it_came() {
    let mut other = with_adapter(false);
    other.avatar = false;
    assert_eq!(
        filter_replayed(&mut other, Some("m1"), &block("Persona.")),
        block("Persona.")
    );
}

#[test]
fn a_persona_saved_while_a_prompt_streams_is_not_settled_by_that_prompts_answer() {
    let (store, directory) = store_with("Be brief.");
    let mut state = with_adapter(false);
    first_prompt(&store, &mut state, "prompt-1");
    // The person changes the persona while prompt-1, which carried the old one, is answered.
    owe(&mut state);
    assert!(state.persona_carried_by.is_none());
    answered(&mut state, "prompt-1", true);
    assert!(state.persona_pending);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_block_whose_end_never_comes_does_not_swallow_what_the_person_wrote_after_it() {
    let filler = "x".repeat(PROMPT_BYTES + BLOCK_END.len());
    let long = format!("{BLOCK_LABEL}\n\n{filler}");
    let shown = shown(&[("m1", &long), ("m2", "Hello")]);
    assert_eq!(shown, [long, "Hello".to_owned()]);
}

#[test]
fn a_load_starts_a_replay_afresh() {
    let (store, directory) = store_with("Be brief.");
    let mut state = with_adapter(false);
    filter_replayed(&mut state, Some("m1"), BLOCK_LABEL);
    assert!(state.persona_replay.is_some());
    state.session_id = Some("native-1".into());
    opening(&store, &directory, &mut state);
    assert!(state.persona_replay.is_none());
    std::fs::remove_dir_all(directory).unwrap();
}
