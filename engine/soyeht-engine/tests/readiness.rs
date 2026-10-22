//! `engine.readiness` through the engine API: the table of `x-readiness` answered for this
//! machine, whatever it has installed.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::json;
use support::Rig;

#[test]
fn the_engine_lists_what_the_machine_has_with_a_command_for_what_it_lacks() {
    let rig = Rig::new();
    let mut client = rig.client();
    let reply = client.call("engine.readiness", json!({}));
    let system = reply["system"].as_str().unwrap();
    assert!(["macos", "debian", "fedora", "windows", "other"].contains(&system));
    let items = reply["items"].as_array().unwrap();
    let git = items.iter().find(|item| item["id"] == "git").unwrap();
    assert_eq!(git["kind"], "tool");
    assert_eq!(git["required"], true);
    assert!(
        items
            .iter()
            .any(|item| item["kind"] == "harness" && item["id"] == "registry.claude-acp"),
        "{reply}"
    );
    for item in items {
        let found = item["found"].as_bool().unwrap();
        assert!(
            !found || item.get("hint").is_none(),
            "a program that is there needs no command: {item}"
        );
        assert!(
            found || system == "other" || item["id"] == "lsof" || item["hint"].is_string(),
            "a missing program has its command: {item}"
        );
    }
}
