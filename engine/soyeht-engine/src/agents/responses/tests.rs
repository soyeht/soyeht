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
fn no_method_is_sent_when_the_person_chose_none() {
    let state = offering(json!([{"id":"login","name":"Login"}]));
    assert_eq!(reauthentication_method(&state), None);
}

#[test]
fn the_chosen_protocol_method_is_sent_while_the_agent_offers_it() {
    let mut state = offering(json!([{"id":"login","name":"Login"}]));
    state.agent_sign_in = Some("login".into());
    assert_eq!(reauthentication_method(&state), Some("login".into()));
}

#[test]
fn a_method_the_agent_no_longer_offers_is_not_sent() {
    let mut state = offering(json!([{"id":"other","name":"Other"}]));
    state.agent_sign_in = Some("login".into());
    assert_eq!(reauthentication_method(&state), None);
    let mut state = offering(json!([]));
    state.agent_sign_in = Some("login".into());
    assert_eq!(reauthentication_method(&state), None);
}

#[test]
fn a_method_offered_only_as_a_terminal_sign_in_is_not_sent() {
    let mut state = offering(json!([{"id":"login","name":"Login","type":"terminal"}]));
    state.agent_sign_in = Some("login".into());
    assert_eq!(reauthentication_method(&state), None);
}

#[test]
fn a_refused_reauthentication_clears_the_choice_and_asks_for_sign_in() {
    let mut state = offering(json!([{"id":"login","name":"Login"}]));
    state.agent_sign_in = Some("login".into());
    refuse_reauthentication(&mut state, &mut pane(), &mut Changes::default()).unwrap();
    assert_eq!(state.agent_sign_in, None);
    assert_eq!(state.phase, Phase::NeedsLogin);
}

#[test]
fn a_refused_authenticate_clears_the_choice_and_an_accepted_one_keeps_it() {
    let refused = soyeht_acp::RpcError {
        code: -32000,
        message: "Authentication required".into(),
        data: None,
    };
    let mut state = offering(json!([{"id":"login","name":"Login"}]));
    state.agent_sign_in = Some("login".into());
    auth_response(&mut state, &pane(), &mut Changes::default(), Ok(json!({}))).unwrap();
    assert_eq!(state.agent_sign_in.as_deref(), Some("login"));
    auth_response(&mut state, &pane(), &mut Changes::default(), Err(refused)).unwrap();
    assert_eq!(state.agent_sign_in, None);
}
