use super::*;

fn params(request: Request) -> Value {
    match request {
        Request::Create(value)
        | Request::Next(value)
        | Request::Show(value)
        | Request::List(value)
        | Request::Update(value)
        | Request::Comment(value)
        | Request::SubitemAdd(value)
        | Request::SubitemDone(value)
        | Request::Block(value)
        | Request::Release(value)
        | Request::Review(value)
        | Request::Complete(value)
        | Request::Dispatch(value)
        | Request::Unblock(value)
        | Request::Move(value)
        | Request::WorktreeClean(value)
        | Request::LoopSet(value) => value,
    }
}

fn assert_valid(request: Request) {
    let (method, value) = match request {
        Request::Create(value) => ("task.create", value),
        Request::Next(value) => ("task.next", value),
        Request::Show(value) => ("task.show", value),
        Request::List(value) => ("task.list", value),
        Request::Update(value) => ("task.update", value),
        Request::Comment(value) => ("task.comment", value),
        Request::SubitemAdd(value) => ("task.subitem.add", value),
        Request::SubitemDone(value) => ("task.subitem.done", value),
        Request::Block(value) => ("task.block", value),
        Request::Release(value) => ("task.release", value),
        Request::Review(value) => ("task.request_review", value),
        Request::Complete(value) => ("task.complete", value),
        Request::Dispatch(value) => ("task.dispatch", value),
        Request::Unblock(value) => ("task.unblock", value),
        Request::Move(value) => ("task.move", value),
        Request::WorktreeClean(value) => ("task.worktree.clean", value),
        Request::LoopSet(value) => ("loop.set", value),
    };
    soyeht_protocol::validate::parameters(method, &value).unwrap();
}

#[test]
fn create_parser_preserves_task_contract_fields() {
    let request = create(
        "workspace",
        "Fix it",
        &[
            "--description",
            "Details",
            "--how-to-verify",
            "Run checks",
            "--must-not-change",
            "Keep pairing",
            "--harness",
            "agent",
            "--priority",
            "high",
            "--create-worktree",
            "true",
            "--permission-mode",
            "ask",
            "--dependencies",
            "WS-1, WS-2",
        ],
        "key",
    )
    .unwrap();
    let value = params(request);
    assert_eq!(value["mustNotChange"], "Keep pairing");
    assert_eq!(value["executor"]["harness"], "agent");
    assert_eq!(value["executor"]["workspaceDefault"], false);
    assert_eq!(value["dependencies"], json!(["WS-1", "WS-2"]));
    soyeht_protocol::validate::parameters("task.create", &value).unwrap();
}

#[test]
fn complete_parser_always_sends_both_declaration_lists() {
    let value = params(complete("workspace", "WS-1", "Done", &[], "key").unwrap());
    assert_eq!(value["contractChanges"], json!([]));
    assert_eq!(value["outOfScopeEdits"], json!([]));
    soyeht_protocol::validate::parameters("task.complete", &value).unwrap();
}

#[test]
fn executor_identity_is_all_or_nothing() {
    let error = comment("workspace", "WS-1", "text", &["--pane", "pane"], "key").unwrap_err();
    assert!(error.to_string().contains("requires --pane"));
}

fn assert_all_valid(cases: Vec<Result<Request>>) {
    for request in cases {
        assert_valid(request.unwrap());
    }
}

#[test]
fn every_task_query_and_authoring_command_builds_valid_parameters() {
    assert_all_valid(vec![
        parse_task(
            &[
                "create",
                "workspace",
                "Title",
                "--must-not-change",
                "Boundary",
            ],
            "key",
        ),
        parse_task(&["next", "workspace", "pane", "7"], "key"),
        parse_task(&["show", "workspace", "WS-1"], "key"),
        parse_task(&["list", "workspace", "--limit", "20"], "key"),
        parse_task(
            &["update", "workspace", "WS-1", "2", "--title", "New title"],
            "key",
        ),
        parse_task(
            &[
                "comment",
                "workspace",
                "WS-1",
                "Milestone",
                "--pane",
                "pane",
                "--generation",
                "7",
                "--attempt",
                "2",
            ],
            "key",
        ),
    ]);
}

#[test]
fn subitem_add_builds_valid_parameters() {
    assert_all_valid(vec![parse_task(
        &[
            "subitem",
            "add",
            "workspace",
            "WS-1",
            "Step",
            "--agent",
            "pane",
            "--pane",
            "pane",
            "--generation",
            "7",
            "--attempt",
            "2",
        ],
        "key",
    )]);
}

#[test]
fn subitem_done_builds_valid_parameters() {
    assert_all_valid(vec![parse_task(
        &[
            "subitem",
            "done",
            "workspace",
            "WS-1",
            "SUB-1",
            "--pane",
            "pane",
            "--generation",
            "7",
            "--attempt",
            "2",
        ],
        "key",
    )]);
}

#[test]
fn task_block_builds_valid_parameters() {
    assert_all_valid(vec![parse_task(
        &[
            "block",
            "workspace",
            "WS-1",
            "dependency",
            "Waiting",
            "--dependency",
            "WS-2",
            "--pane",
            "pane",
            "--generation",
            "7",
            "--attempt",
            "2",
        ],
        "key",
    )]);
}

#[test]
fn remaining_lifecycle_coordination_and_loop_commands_build_valid_parameters() {
    assert_all_valid(vec![
        parse_task(
            &[
                "release",
                "workspace",
                "WS-1",
                "--pane",
                "pane",
                "--generation",
                "7",
                "--attempt",
                "2",
            ],
            "key",
        ),
        parse_task(
            &[
                "request-review",
                "workspace",
                "WS-1",
                "--pane",
                "pane",
                "--generation",
                "7",
                "--attempt",
                "2",
            ],
            "key",
        ),
        parse_task(
            &[
                "complete",
                "workspace",
                "WS-1",
                "Done",
                "--contract-changes",
                "[]",
                "--out-of-scope-edits",
                "[]",
                "--pane",
                "pane",
                "--generation",
                "7",
                "--attempt",
                "2",
            ],
            "key",
        ),
        parse_task(
            &[
                "dispatch",
                "workspace",
                "WS-1",
                "--harness",
                "agent",
                "--permission-mode",
                "ask",
            ],
            "key",
        ),
        parse_task(&["unblock", "workspace", "WS-1"], "key"),
        parse_task(
            &["move", "workspace", "WS-1", "cancelled", "Stopped"],
            "key",
        ),
        parse_task(&["worktree-clean", "workspace", "WS-1"], "key"),
        parse_loop(
            &[
                "set",
                "pane",
                "Pull work",
                "30",
                "10",
                "three_empty",
                "true",
            ],
            "key",
        ),
    ]);
}
