#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use soyeht_protocol::{SCHEMA, setting};

fn contract() -> serde_json::Map<String, Value> {
    let schema: Value = serde_json::from_str(SCHEMA).unwrap();
    schema["x-settings"].as_object().unwrap().clone()
}

/// A key of the contract with its family parameter filled in.
fn concrete(key: &str) -> String {
    key.replace("<harnessId>", "test-agent")
}

#[test]
fn every_setting_of_the_contract_has_its_default_and_no_other_exists() {
    let contract = contract();
    for (key, rule) in &contract {
        let key = concrete(key);
        assert_eq!(
            setting::default_value(&key).unwrap(),
            rule["default"],
            "{key}"
        );
        assert_eq!(setting::validate(&key, &rule["default"]), Ok(()), "{key}");
    }
    let alone = contract.keys().filter(|key| !key.contains('<')).count();
    assert_eq!(setting::KEYS.len(), alone);
    for key in setting::KEYS.iter().copied() {
        assert!(contract.contains_key(key), "{key}");
    }
    for key in [
        "web.openLocalLinksInSoyeht",
        "harness",
        "harness.defaultMode.",
    ] {
        assert_eq!(
            setting::default_value(key),
            Err(setting::SettingError::Unknown)
        );
        assert_eq!(
            setting::validate(key, &json!(true)),
            Err(setting::SettingError::Unknown)
        );
    }
}

#[test]
fn the_bounds_of_the_contract_decide_what_is_accepted() {
    let contract = contract();
    for (key, rule) in &contract {
        let key = concrete(key);
        match rule["type"].as_str().unwrap() {
            "integer" => {
                let (low, high) = (
                    rule["minimum"].as_i64().unwrap(),
                    rule["maximum"].as_i64().unwrap(),
                );
                for good in [low, high] {
                    assert_eq!(
                        setting::validate(&key, &json!(good)),
                        Ok(()),
                        "{key} {good}"
                    );
                }
                for bad in [
                    json!(low - 1),
                    json!(high + 1),
                    json!("80"),
                    json!(1.5),
                    json!(true),
                ] {
                    assert_eq!(
                        setting::validate(&key, &bad),
                        Err(setting::SettingError::Invalid),
                        "{key} {bad}"
                    );
                }
            }
            "boolean" => {
                assert_eq!(setting::validate(&key, &json!(false)), Ok(()), "{key}");
                assert_eq!(
                    setting::validate(&key, &json!("true")),
                    Err(setting::SettingError::Invalid),
                    "{key}"
                );
            }
            _ => {
                let limit = rule["maxLength"].as_u64().unwrap() as usize;
                assert_eq!(
                    setting::validate(&key, &json!("x".repeat(limit))),
                    Ok(()),
                    "{key}"
                );
                for bad in [json!("x".repeat(limit + 1)), json!("a\0b"), json!(7)] {
                    assert_eq!(
                        setting::validate(&key, &bad),
                        Err(setting::SettingError::Invalid),
                        "{key} {bad}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_family_key_names_its_parameter_within_the_bounds() {
    let bounds = &contract()["harness.defaultMode.<harnessId>"]["parameter"];
    let (low, high) = (
        bounds["minLength"].as_u64().unwrap() as usize,
        bounds["maxLength"].as_u64().unwrap() as usize,
    );
    let key = setting::harness_default_mode_key("codex");
    assert_eq!(
        setting::harness_default_mode_key_parameter(&key),
        Some("codex")
    );
    assert_eq!(
        setting::harness_default_mode_key_parameter(setting::HARNESS_PREFERRED_ID),
        None
    );
    for length in [low, high] {
        let id = "h".repeat(length);
        let key = setting::harness_default_mode_key(&id);
        assert_eq!(
            setting::harness_default_mode_key_parameter(&key),
            Some(id.as_str())
        );
    }
    for length in [low - 1, high + 1] {
        let key = setting::harness_default_mode_key(&"h".repeat(length));
        assert_eq!(setting::harness_default_mode_key_parameter(&key), None);
        assert_eq!(
            setting::default_value(&key),
            Err(setting::SettingError::Unknown)
        );
    }
}
