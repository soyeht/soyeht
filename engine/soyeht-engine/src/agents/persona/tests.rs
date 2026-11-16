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
    Avatar, AvatarBinding, PaneMode, Workspace,
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
    let avatar = Avatar {
        avatar_id: "a".into(),
        name: "Sabio".into(),
        kind: "uploaded".into(),
        source_hash: "a".repeat(64),
        image_provider: None,
        image_model: None,
        voice: None,
        created_at: "2026-11-15T10:00:00Z".into(),
    };
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
    let first = ahead_of(&store, &mut state, "w", person.clone()).unwrap();
    assert_eq!(
        first["prompt"],
        json!([
            {"type": "text", "text": block("You are Sabio, the avatar agent of this project.\n\nBe brief.")},
            {"type": "text", "text": "Hello"}
        ])
    );
    assert!(!state.persona_pending);
    assert_eq!(
        ahead_of(&store, &mut state, "w", person.clone()).unwrap(),
        person
    );
    std::fs::remove_dir_all(directory).unwrap();
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
    let sent = ahead_of(&MemoryStore::default(), &mut state, "w", person.clone()).unwrap();
    assert_eq!(sent, person);
    assert!(!state.persona_pending);
}

#[test]
fn a_replayed_user_message_loses_the_block_the_engine_put_ahead_of_it() {
    let avatar = with_adapter(false);
    let text = block("You are Sabio, the avatar agent of this project.");
    // As two messages of the replay, and as one joined to the prompt's own text.
    assert_eq!(strip_replayed(&avatar, &text), "");
    assert_eq!(strip_replayed(&avatar, "Hello"), "Hello");
    assert_eq!(strip_replayed(&avatar, &format!("{text}\nHello")), "Hello");
    // Another persona, an older one, goes as well.
    assert_eq!(strip_replayed(&avatar, &block("Another persona.")), "");
    // What a person wrote that only starts alike stays, and a pane that is not an avatar's is left alone.
    assert_eq!(strip_replayed(&avatar, BLOCK_LABEL), BLOCK_LABEL);
    let mut other = with_adapter(false);
    other.avatar = false;
    assert_eq!(strip_replayed(&other, &text), text);
}
