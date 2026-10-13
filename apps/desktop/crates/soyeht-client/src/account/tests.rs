//! The account rules, against the vectors the Mac passes as well
//! (`contracts/vectors/agent-account.json`).

use super::*;
use serde_json::json;

use crate::testing::{agent_session as agent, vector};

#[test]
fn the_account_section_follows_the_shared_vectors() {
    let vectors = vector("agent-account");
    assert_eq!(SECTION_TITLE, vectors["copy"]["sectionTitle"]);
    assert_eq!(EXPLANATION, vectors["copy"]["explanation"]);
    for case in vectors["account"].as_array().unwrap() {
        let mut fields =
            json!({"reauthMethods": case["reauthMethods"], "canSignOut": case["canSignOut"]});
        if case["operation"] == true {
            fields["operation"] = json!({
                "operationId": "o", "configId": "model", "value": "m",
                "startedAt": "2026-10-01T00:00:00Z",
            });
        }
        let session = agent(fields);
        let state: PaneState = serde_json::from_value(case["paneState"].clone()).unwrap();
        let actions = AccountActions::new(
            Some(&session),
            Some(&state),
            case["connected"].as_bool().unwrap(),
        );
        let name = &case["name"];
        let Some(expected) = case["expected"].as_object() else {
            assert_eq!(actions, None, "{name}");
            continue;
        };
        let actions = actions.unwrap();
        assert_eq!(actions.enabled, expected["enabled"], "{name}");
        let items: Vec<(String, String)> = actions
            .items()
            .into_iter()
            .map(|item| {
                let action = match item.action {
                    AccountAction::SwitchTo(id) => format!("switch:{id}"),
                    AccountAction::SignOut => "signOut".to_owned(),
                };
                (action, item.title)
            })
            .collect();
        let wanted: Vec<(String, String)> = expected["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| {
                (
                    pair[0].as_str().unwrap().to_owned(),
                    pair[1].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        assert_eq!(items, wanted, "{name}");
    }
    assert_eq!(AccountActions::new(None, None, true), None);
}
