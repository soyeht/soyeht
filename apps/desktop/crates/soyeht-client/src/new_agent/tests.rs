use super::*;
use serde_json::Value;

const VECTORS: &str = include_str!("../../../../../../contracts/vectors/new-pane-form.json");

fn vector() -> Value {
    serde_json::from_str(VECTORS).unwrap()
}

/// The harnesses a case lists, by name from the vector's catalogue.
fn harnesses(vector: &Value, case: &Value) -> Vec<HarnessInfo> {
    case["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| {
            let entry = vector["catalogue"][name.as_str().unwrap()].clone();
            serde_json::from_value(entry).unwrap()
        })
        .collect()
}

fn text(case: &Value, key: &str) -> String {
    case[key].as_str().unwrap_or_default().to_owned()
}

#[test]
fn what_stops_an_agent_from_starting_is_what_the_vector_says() {
    let vector = vector();
    let cases = vector["blockers"].as_array().unwrap();
    assert!(cases.len() > 5, "the vector file lost its cases");
    for case in cases {
        let prompt = match case["promptBytes"].as_str() {
            Some("limit") => "a".repeat(PROMPT_BYTES),
            Some("limitPlusOne") => "a".repeat(PROMPT_BYTES + 1),
            _ => text(case, "prompt"),
        };
        let form = NewAgent {
            directory: text(case, "directory"),
            harness: text(case, "harness"),
            prompt,
        };
        let expected = match case["blocker"].as_str() {
            None => None,
            Some("noFolder") => Some(Blocker::NoFolder),
            Some("noAgent") => Some(Blocker::NoAgent),
            Some("agentUnavailable") => Some(Blocker::AgentUnavailable),
            Some("promptTooLong") => Some(Blocker::PromptTooLong),
            Some(other) => panic!("unknown blocker {other}"),
        };
        assert_eq!(
            form.blocker(&harnesses(&vector, case)),
            expected,
            "{}",
            case["name"]
        );
    }
}

#[test]
fn the_first_message_is_sent_without_the_blank_space_around_it() {
    let vector = vector();
    for case in vector["firstPrompts"].as_array().unwrap() {
        let form = NewAgent {
            prompt: text(case, "prompt"),
            ..NewAgent::default()
        };
        assert_eq!(form.first_prompt(), text(case, "first"), "{}", case["name"]);
    }
}

#[test]
fn the_chosen_agent_stays_while_it_can_run_and_otherwise_the_default_takes_its_place() {
    let vector = vector();
    for case in vector["keepOrChoose"].as_array().unwrap() {
        let mut form = NewAgent {
            harness: text(case, "harness"),
            ..NewAgent::default()
        };
        form.keep_or_choose_harness(&harnesses(&vector, case), case["preferred"].as_str());
        assert_eq!(form.harness, text(case, "expected"), "{}", case["name"]);
    }
}

#[test]
fn every_blocker_has_a_sentence() {
    for blocker in [
        Blocker::NoFolder,
        Blocker::NoAgent,
        Blocker::AgentUnavailable,
        Blocker::PromptTooLong,
    ] {
        assert!(blocker.explanation().ends_with('.'));
    }
}
