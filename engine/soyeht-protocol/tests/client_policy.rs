#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::Value;
use soyeht_protocol::{ConfigOption, UsageProvider};

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
