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
