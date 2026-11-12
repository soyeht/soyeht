use super::*;
use crate::agents::state::tests::state;
use soyeht_protocol::PaneMode;

fn offering(methods: Value) -> State {
    let mut state = state();
    state.initialized =
        Some(Initialization::parse(&json!({"protocolVersion":1,"authMethods":methods})).unwrap());
    state
}

fn pane() -> StoredPane {
    let pane = crate::test_panes::pane("p", "w", PaneMode::Agent, "/tmp");
    StoredPane {
        pane,
        closed: false,
        instance_key: "key".into(),
        instance_id: None,
        supervisor_id: None,
        sequence_base: 0,
    }
}

#[test]
fn a_refused_reauthentication_forgets_the_harness_sign_in_and_asks_for_sign_in() {
    let mut state = offering(json!([{"id":"login","name":"Login"}]));
    let mut changes = Changes::default();
    refuse_reauthentication(&mut state, &mut pane(), &mut changes).unwrap();
    assert_eq!(
        changes.sign_in,
        Some(SignInChange::forget(&state.launch.id))
    );
    assert_eq!(state.phase, Phase::NeedsLogin);
}

#[test]
fn a_refused_authenticate_forgets_the_harness_sign_in_and_an_accepted_one_keeps_it() {
    let refused = soyeht_acp::RpcError {
        code: -32000,
        message: "Authentication required".into(),
        data: None,
    };
    let mut state = offering(json!([{"id":"login","name":"Login"}]));
    let mut accepted = Changes::default();
    auth_response(&mut state, &pane(), &mut accepted, Ok(json!({}))).unwrap();
    assert_eq!(accepted.sign_in, None);
    let mut rejected = Changes::default();
    auth_response(&mut state, &pane(), &mut rejected, Err(refused)).unwrap();
    assert_eq!(
        rejected.sign_in,
        Some(SignInChange::forget(&state.launch.id))
    );
}

/// An agent whose `session/new` is open and has held an update, over a store of its own.
fn opening_with_a_held_update() -> (soyeht_store::database::Store, std::path::PathBuf, State) {
    let (store, directory) = crate::agents::state::tests::scratch_store();
    let mut state = offering(json!([]));
    state.mcp_enabled = false;
    state.held_updates.push(json!({"sessionId":"old"}));
    state.held_overflowed = true;
    (store, directory, state)
}

#[test]
fn a_new_open_forgets_the_updates_held_for_the_last_one() {
    let (store, directory, mut state) = opening_with_a_held_update();
    let runtime = Runtime {
        data_directory: &directory,
        now_ms: 0,
        timestamp: "now",
    };
    open(
        &store,
        &runtime,
        &mut state,
        &pane(),
        &mut Changes::default(),
    )
    .unwrap();
    assert_eq!(state.phase, Phase::Opening);
    assert!(state.held_updates.is_empty() && !state.held_overflowed);
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn an_error_to_session_new_forgets_the_updates_held_for_it() {
    let (store, directory, mut state) = opening_with_a_held_update();
    state.phase = Phase::Opening;
    let runtime = Runtime {
        data_directory: &directory,
        now_ms: 0,
        timestamp: "now",
    };
    let refused = soyeht_acp::RpcError {
        code: -32603,
        message: "Could not start".into(),
        data: None,
    };
    let result = session_response(
        &store,
        &runtime,
        &mut state,
        &mut pane(),
        &Purpose::Open,
        &mut Changes::default(),
        Err(refused),
    );
    assert!(result.is_err());
    assert!(state.held_updates.is_empty());
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_stop_reason_the_engine_does_not_know_is_refused_not_passed_on() {
    assert_eq!(
        decode_stop_reason(&json!({"stopReason":"end_turn"})).unwrap(),
        TurnStopReason::EndTurn
    );
    // The ACP layer already names only the reasons it recognises; a reason it let through that the
    // contract lacks is refused here, as an internal error.
    assert!(decode_stop_reason(&json!({"stopReason":"stop_everything"})).is_err());
    assert!(
        serde_json::from_value::<TurnStopReason>(Value::String("stop_everything".into())).is_err()
    );
}
