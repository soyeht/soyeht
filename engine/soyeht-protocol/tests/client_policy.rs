#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::Value;
use soyeht_protocol::{ConfigOption, PaneState, UsageProvider, client_policy::StateDot};

fn vector() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/vectors/usage.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The cases of `contracts/vectors/usage.json`, which the Mac client passes as well.
#[test]
fn a_selector_has_the_roles_the_vector_says() {
    for case in vector()["configRoles"].as_array().unwrap() {
        let option: ConfigOption = serde_json::from_value(case["option"].clone()).unwrap();
        let mut roles = Vec::new();
        for (role, has) in [
            ("model", option.is_model()),
            ("effort", option.is_effort()),
            ("permission", option.is_permission_mode()),
        ] {
            if has {
                roles.push(role);
            }
        }
        assert_eq!(
            Value::from(roles),
            case["roles"],
            "{}",
            case["name"].as_str().unwrap()
        );
    }
}

#[test]
fn every_provider_has_a_name() {
    let names: Vec<_> = [
        UsageProvider::Claude,
        UsageProvider::Codex,
        UsageProvider::Cursor,
    ]
    .iter()
    .map(UsageProvider::display_name)
    .collect();
    assert_eq!(names, ["Claude", "Codex", "Cursor"]);
}

fn vector_named(name: &str) -> Value {
    let path = format!(
        "{}/../../contracts/vectors/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The cases of `contracts/vectors/pane-state-presentation.json`, which the Mac client passes
/// as well.
#[test]
fn every_pane_state_is_presented_as_the_vector_says() {
    let vector = vector_named("pane-state-presentation");
    let cases = vector["states"].as_array().unwrap();
    for case in cases {
        let wire = case["state"].as_str().unwrap();
        let state: PaneState = serde_json::from_value(Value::from(wire)).unwrap();
        assert_eq!(state.as_str(), wire);
        assert_eq!(
            state.is_turn_running(),
            case["turnRunning"].as_bool().unwrap(),
            "{wire}"
        );
        let dot = match state.dot() {
            StateDot::Success => "success",
            StateDot::Accent => "accent",
            StateDot::Warning => "warning",
            StateDot::Muted => "muted",
            StateDot::Secondary => "secondary",
        };
        assert_eq!(dot, case["dot"].as_str().unwrap(), "{wire}");
    }
    let schema: Value = serde_json::from_str(soyeht_protocol::SCHEMA).unwrap();
    assert_eq!(
        cases.len(),
        schema["$defs"]["PaneState"]["enum"]
            .as_array()
            .unwrap()
            .len()
    );
}
