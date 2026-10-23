//! The picker's rules, against the vectors the Mac's `HarnessPickerModel` passes as well
//! (`contracts/vectors/harness-picker.json`).

use super::*;
use crate::testing::strings;
use serde_json::{Value, json};

fn vectors() -> Value {
    crate::testing::vector("harness-picker")
}

fn harness_named(vectors: &Value, key: &str) -> HarnessInfo {
    serde_json::from_value(vectors["catalogue"][key].clone()).unwrap()
}

fn catalogue(vectors: &Value, keys: &Value) -> Vec<HarnessInfo> {
    keys.as_array()
        .unwrap()
        .iter()
        .map(|key| harness_named(vectors, key.as_str().unwrap()))
        .collect()
}

fn ids(items: &[&HarnessInfo]) -> Vec<String> {
    items.iter().map(|harness| harness.id.clone()).collect()
}

#[test]
fn layouts_follow_the_shared_vectors() {
    let vectors = vectors();
    for case in vectors["layouts"].as_array().unwrap() {
        let harnesses = catalogue(&vectors, &case["harnesses"]);
        let layout = layout(
            &harnesses,
            case["query"].as_str().unwrap(),
            case["excluding"].as_str(),
        );
        let sections: Vec<(String, Vec<String>)> = layout
            .sections
            .iter()
            .map(|(group, items)| (group.title().to_owned(), ids(items)))
            .collect();
        let expected: Vec<(String, Vec<String>)> = case["sections"]
            .as_array()
            .unwrap()
            .iter()
            .map(|section| {
                (
                    section[0].as_str().unwrap().to_owned(),
                    strings(&section[1]),
                )
            })
            .collect();
        assert_eq!(sections, expected, "{}", case["name"]);
        assert_eq!(
            ids(&layout.more),
            strings(&case["more"]),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn defaults_follow_the_shared_vectors() {
    let vectors = vectors();
    for case in vectors["defaults"].as_array().unwrap() {
        let harnesses = catalogue(&vectors, &case["harnesses"]);
        let chosen = default_harness(
            &harnesses,
            case["excluding"].as_str(),
            case["preferred"].as_str(),
        );
        assert_eq!(
            chosen.map(|harness| harness.id.as_str()),
            case["expected"].as_str(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn statuses_follow_the_shared_vectors() {
    let vectors = vectors();
    for case in vectors["statuses"].as_array().unwrap() {
        let harness = harness_named(&vectors, case["harness"].as_str().unwrap());
        assert_eq!(status_label(&harness), case["label"], "{}", case["harness"]);
        assert_eq!(
            status_reason(&harness),
            case["reason"],
            "{}",
            case["harness"]
        );
        assert_eq!(
            picker_notice(&harness),
            case["notice"].as_str(),
            "{}",
            case["harness"]
        );
        assert_eq!(
            is_installed(&harness),
            case["installed"],
            "{}",
            case["harness"]
        );
    }
}

#[test]
fn a_reason_the_engine_gave_wins_over_the_one_the_state_implies() {
    let vectors = vectors();
    for case in vectors["reasons"].as_array().unwrap() {
        let mut harness = harness_named(&vectors, "tool");
        harness.setup_reason = Some(serde_json::from_value(case["setupReason"].clone()).unwrap());
        assert_eq!(
            status_reason(&harness),
            case["text"],
            "{}",
            case["setupReason"]
        );
    }
}

#[test]
fn short_names_and_the_switch_label_follow_the_shared_vectors() {
    let vectors = vectors();
    for pair in vectors["shortNames"].as_array().unwrap() {
        assert_eq!(short_name(pair[0].as_str().unwrap()), pair[1]);
    }
    let labels = &vectors["actionLabels"];
    let target = HarnessInfo {
        name: labels["target"].as_str().unwrap().to_owned(),
        ..harness_named(&vectors, "codex")
    };
    assert_eq!(switch_label(Some(&target)), labels["withTarget"]);
    assert_eq!(switch_label(None), labels["withoutTarget"]);
}

#[test]
fn a_model_the_agent_refused_says_so() {
    let model = |refusal: Value| -> ModelInfo {
        serde_json::from_value(json!({"id": "m", "name": "Opus", "observedRefusal": refusal}))
            .unwrap()
    };
    let refused = model(json!({"reason": "no access", "observedAt": "2026-10-01T00:00:00Z"}));
    assert_eq!(model_label(&refused), "Opus \u{2014} refused");
    let fine: ModelInfo = serde_json::from_value(json!({"id": "m", "name": "Opus"})).unwrap();
    assert_eq!(model_label(&fine), "Opus");
}

#[test]
fn more_opens_when_asked_while_searching_when_alone_or_when_it_holds_the_choice() {
    let vectors = vectors();
    let installed = catalogue(&vectors, &json!(["tool", "alpha"]));
    let with_installed = layout(&installed, "", None);
    assert!(!more_expanded(&with_installed, false, false, None));
    assert!(more_expanded(&with_installed, true, false, None));
    assert!(more_expanded(&with_installed, false, true, None));
    assert!(more_expanded(
        &with_installed,
        false,
        false,
        Some("registry.a")
    ));
    assert!(!more_expanded(
        &with_installed,
        false,
        false,
        Some("local.tool")
    ));
    let only_more = catalogue(&vectors, &json!(["alpha"]));
    assert!(more_expanded(
        &layout(&only_more, "", None),
        false,
        false,
        None
    ));
}

#[test]
fn identifiers_and_the_chip_follow_the_shared_vectors() {
    let vectors = vectors();
    for pair in vectors["readableNames"].as_array().unwrap() {
        assert_eq!(readable_name(pair[0].as_str().unwrap()), pair[1]);
    }
    for case in vectors["chipTitles"].as_array().unwrap() {
        let harnesses = catalogue(&vectors, &case["harnesses"]);
        let (title, busy) = chip_title(
            &harnesses,
            case["current"].as_str().unwrap(),
            case["switchingTo"].as_str(),
            case["paneName"].as_str(),
        );
        assert_eq!(title, case["title"], "{}", case["name"]);
        assert_eq!(busy, case["busy"], "{}", case["name"]);
    }
}
