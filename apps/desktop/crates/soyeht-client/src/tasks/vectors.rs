//! The cases of `contracts/vectors/tasks.json`, which the Mac client passes as well, and the
//! replica's rules.

use super::*;
use crate::testing::{cases as listed, text, vector};
use serde_json::{Value, json};
use soyeht_protocol::{
    Attempt, AttemptResult, BlockingKind, Comment, ContractChange, OutOfScopeEdit, Subitem, Task,
    TaskExecutor, TaskPriority, TaskReviewBadge, TaskShowResult, TaskState,
};
use std::collections::HashMap;

fn cases(key: &str) -> Vec<Value> {
    listed("tasks", key)
}

fn tasks() -> HashMap<String, Task> {
    cases("tasks")
        .into_iter()
        .map(|value| {
            let task: Task = serde_json::from_value(value).unwrap();
            (task.task_id.clone(), task)
        })
        .collect()
}

fn pick<'a>(ids: &Value, held: &'a HashMap<String, Task>) -> Vec<&'a Task> {
    ids.as_array()
        .unwrap()
        .iter()
        .map(|id| &held[text(id)])
        .collect()
}

fn state(name: &Value) -> TaskState {
    serde_json::from_value(name.clone()).unwrap()
}

fn from<T: serde::de::DeserializeOwned>(value: &Value) -> T {
    serde_json::from_value(value.clone()).unwrap()
}

#[test]
fn the_board_and_the_list_sort_tasks_the_way_the_mac_does() {
    let held = tasks();
    for case in cases("stateTitles") {
        let value = state(&case["state"]);
        assert_eq!(state_title(&value), text(&case["title"]));
        let role = format!("{:?}", state_role(&value)).to_lowercase();
        assert_eq!(role, text(&case["role"]));
    }
    for case in cases("columns") {
        let chosen = pick(&case["taskIds"], &held);
        let board: Vec<TaskState> = case["board"]
            .as_array()
            .unwrap()
            .iter()
            .map(state)
            .collect();
        let list: Vec<TaskState> = case["list"].as_array().unwrap().iter().map(state).collect();
        assert_eq!(board_states(&chosen), board, "{}", case["name"]);
        assert_eq!(list_states(&chosen), list);
    }
    let order = vector("tasks")["listOrder"].clone();
    let mut sorted = pick(&order["taskIds"], &held);
    sorted.sort_by(|left, right| list_order(left, right));
    let ids: Vec<&str> = sorted.iter().map(|task| task.task_id.as_str()).collect();
    let expected: Vec<&str> = order["sorted"]
        .as_array()
        .unwrap()
        .iter()
        .map(text)
        .collect();
    assert_eq!(ids, expected);
    for case in cases("priorityTitles") {
        assert_eq!(
            priority_title(&from::<TaskPriority>(&case["priority"])),
            text(&case["title"])
        );
    }
    for case in cases("badges") {
        let badge: TaskReviewBadge = from(&case["badge"]);
        assert_eq!(badge_title(&badge), text(&case["title"]));
        assert_eq!(
            format!("{:?}", badge_role(&badge)).to_lowercase(),
            text(&case["role"])
        );
    }
    for case in cases("blockingTitles") {
        assert_eq!(
            blocking_title(&from::<BlockingKind>(&case["kind"])),
            text(&case["title"])
        );
    }
    for case in cases("attemptTitles") {
        assert_eq!(
            attempt_result_title(&from::<AttemptResult>(&case["result"])),
            text(&case["title"])
        );
    }
    for case in cases("ownerPlates") {
        let plate = format!("{:?}", owner_plate(text(&case["name"]))).to_lowercase();
        assert_eq!(plate, text(&case["plate"]), "{case}");
    }
}

#[test]
fn a_card_says_where_a_task_stands_and_the_summary_counts_them() {
    let held = tasks();
    let all = vector("tasks");
    for case in cases("statusLines") {
        let task = &held[text(&case["taskId"])];
        assert_eq!(status_line(task), text(&case["line"]));
        assert_eq!(
            format!("{:?}", indicator(task)).to_lowercase(),
            text(&case["indicator"])
        );
    }
    for case in cases("summary") {
        let attention = case["attention"].as_u64().unwrap() as usize;
        assert_eq!(
            summary(&pick(&case["taskIds"], &held), attention),
            text(&case["text"])
        );
    }
    let dispatch = &all["dispatch"];
    let listed = pick(&dispatch["taskIds"], &held);
    let ready: Vec<&str> = ready_to_dispatch(&listed)
        .iter()
        .map(|t| t.task_id.as_str())
        .collect();
    let expected: Vec<&str> = dispatch["ready"]
        .as_array()
        .unwrap()
        .iter()
        .map(text)
        .collect();
    assert_eq!(ready, expected);
    for task in listed {
        assert_eq!(
            dispatch_label(task),
            text(&dispatch["labels"][&task.task_id])
        );
    }
    assert_eq!(NO_TASK_READY, text(&dispatch["none"]));
    assert_eq!(EMPTY_TITLE, text(&all["words"]["emptyTitle"]));
    assert_eq!(EMPTY_DETAIL, text(&all["words"]["emptyDetail"]));
}

#[test]
fn the_detail_names_the_runner_the_subitems_the_attempts_and_the_timeline() {
    let held = tasks();
    let all = vector("tasks");
    for case in cases("runners") {
        let executor: Option<TaskExecutor> = from(&case["executor"]);
        assert_eq!(runner_name(executor.as_ref()), text(&case["name"]));
    }
    for case in cases("subitems") {
        let items: Vec<Subitem> = case["states"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, state)| {
                from(&json!({"subitemId": format!("s{index}"), "taskId": "WS-5", "text": "t", "state": state}))
            })
            .collect();
        assert_eq!(subitems_title(&items), text(&case["title"]));
    }
    for case in cases("attempts") {
        let attempt: Attempt = from(&case["attempt"]);
        assert_eq!(attempt_line(&attempt), text(&case["line"]));
        assert_eq!(attempt_outcome(&attempt), text(&case["outcome"]));
    }
    let wanted = &all["timeline"];
    let detail: TaskShowResult = from(&json!({
        "task": held["WS-5"], "subitems": [], "attempts": [],
        "history": wanted["history"], "comments": wanted["comments"]
    }));
    let entries = timeline(&detail);
    let expected = wanted["entries"].as_array().unwrap();
    assert_eq!(entries.len(), expected.len());
    for (entry, wanted) in entries.iter().zip(expected) {
        assert_eq!(entry.id, text(&wanted["id"]));
        assert_eq!(entry.author.as_deref(), wanted["author"].as_str());
        assert_eq!(entry.text, text(&wanted["text"]));
        assert_eq!(entry.at, text(&wanted["at"]));
    }
    let comments: Vec<Comment> = detail.comments.clone();
    let findings: Vec<&str> = wanted["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(text)
        .collect();
    assert_eq!(drift_findings(&comments), findings);
    let declarations = &all["declarations"];
    let change: ContractChange = from(&declarations["contractChange"]);
    assert_eq!(
        contract_change(&change),
        text(&declarations["contractChange"]["text"])
    );
    let edit: OutOfScopeEdit = from(&declarations["outOfScope"]);
    assert_eq!(
        out_of_scope_edit(&edit),
        text(&declarations["outOfScope"]["text"])
    );
}

#[test]
fn the_new_task_form_says_what_is_missing() {
    let form = vector("tasks")["newTask"].clone();
    for case in form["missing"].as_array().unwrap() {
        let missing = missing_required_fields(
            text(&case["title"]),
            text(&case["description"]),
            text(&case["mustNotChange"]),
        );
        let expected: Vec<&str> = case["missing"]
            .as_array()
            .unwrap()
            .iter()
            .map(text)
            .collect();
        assert_eq!(missing, expected);
    }
    for case in form["hints"].as_array().unwrap() {
        let missing: Vec<&str> = case["missing"]
            .as_array()
            .unwrap()
            .iter()
            .map(text)
            .collect();
        let hint = action_hint(
            &missing,
            case["workspaceDefault"].as_bool().unwrap(),
            text(&case["harness"]),
            text(&case["shortcuts"]),
        );
        assert_eq!(hint, text(&case["text"]));
    }
    for case in form["dependencies"].as_array().unwrap() {
        let ids: Vec<String> = from(&case["dependencies"]);
        assert_eq!(dependency_title(&ids), text(&case["text"]));
    }
    for case in form["runnerTitles"].as_array().unwrap() {
        let title = runner_title(
            case["workspaceDefault"].as_bool().unwrap(),
            case["harnessName"].as_str(),
            case["modelName"].as_str(),
            case["effortName"].as_str(),
        );
        assert_eq!(title, text(&case["text"]));
    }
}
