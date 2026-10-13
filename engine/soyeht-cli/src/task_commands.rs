use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};
use soyeht_cli::{Client, Error, Result};
use soyeht_protocol::{method::*, *};
use std::collections::BTreeMap;

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidArguments(message.into())
}

fn rpc<T: DeserializeOwned + Serialize>(
    client: &mut Client,
    method: &str,
    params: Value,
) -> Result<Value> {
    Ok(serde_json::to_value(client.request::<T>(method, params)?)?)
}

struct Flags {
    values: BTreeMap<String, String>,
}

impl Flags {
    fn parse(args: &[&str], allowed: &[&str]) -> Result<Self> {
        let mut values = BTreeMap::new();
        let mut index = 0;
        while index < args.len() {
            let name = args[index];
            if !allowed.contains(&name) {
                return Err(invalid(format!("Unknown option: {name}")));
            }
            let value = args
                .get(index + 1)
                .ok_or_else(|| invalid(format!("Missing value for {name}")))?;
            if values
                .insert(name.to_owned(), (*value).to_owned())
                .is_some()
            {
                return Err(invalid(format!("Repeated option: {name}")));
            }
            index += 2;
        }
        Ok(Self { values })
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    fn required(&self, name: &str) -> Result<&str> {
        self.get(name)
            .ok_or_else(|| invalid(format!("Missing required option: {name}")))
    }

    fn number(&self, name: &str) -> Result<Option<u32>> {
        self.get(name).map(|value| number(value, name)).transpose()
    }

    fn boolean(&self, name: &str) -> Result<Option<bool>> {
        self.get(name).map(|value| boolean(value, name)).transpose()
    }
}

#[derive(Debug)]
enum Request {
    Create(Value),
    Next(Value),
    Show(Value),
    List(Value),
    Update(Value),
    Comment(Value),
    SubitemAdd(Value),
    SubitemDone(Value),
    Block(Value),
    Release(Value),
    Review(Value),
    Complete(Value),
    Dispatch(Value),
    Unblock(Value),
    Move(Value),
    WorktreeClean(Value),
    LoopSet(Value),
}

impl Request {
    fn run(self, client: &mut Client) -> Result<Value> {
        match self {
            Self::Create(params) => rpc::<TaskCreateResult>(client, TASK_CREATE, params),
            Self::Next(params) => rpc::<TaskNextResult>(client, TASK_NEXT, params),
            Self::Show(params) => rpc::<TaskShowResult>(client, TASK_SHOW, params),
            Self::List(params) => rpc::<TaskListResult>(client, TASK_LIST, params),
            Self::Update(params) => rpc::<TaskUpdateResult>(client, TASK_UPDATE, params),
            Self::Comment(params) => rpc::<TaskCommentResult>(client, TASK_COMMENT, params),
            Self::SubitemAdd(params) => {
                rpc::<TaskSubitemAddResult>(client, TASK_SUBITEM_ADD, params)
            }
            Self::SubitemDone(params) => {
                rpc::<TaskSubitemDoneResult>(client, TASK_SUBITEM_DONE, params)
            }
            Self::Block(params) => rpc::<TaskBlockResult>(client, TASK_BLOCK, params),
            Self::Release(params) => rpc::<TaskReleaseResult>(client, TASK_RELEASE, params),
            Self::Review(params) => {
                rpc::<TaskRequestReviewResult>(client, TASK_REQUEST_REVIEW, params)
            }
            Self::Complete(params) => rpc::<TaskCompleteResult>(client, TASK_COMPLETE, params),
            Self::Dispatch(params) => rpc::<TaskDispatchResult>(client, TASK_DISPATCH, params),
            Self::Unblock(params) => rpc::<TaskUnblockResult>(client, TASK_UNBLOCK, params),
            Self::Move(params) => rpc::<TaskMoveResult>(client, TASK_MOVE, params),
            Self::WorktreeClean(params) => {
                rpc::<TaskWorktreeCleanResult>(client, TASK_WORKTREE_CLEAN, params)
            }
            Self::LoopSet(params) => rpc::<LoopSetResult>(client, LOOP_SET, params),
        }
    }
}

pub fn execute_task(client: &mut Client, args: &[&str], key: &str) -> Result<Value> {
    parse_task(args, key)?.run(client)
}

fn parse_task(args: &[&str], key: &str) -> Result<Request> {
    let request = match args {
        ["create", workspace, title, rest @ ..] => create(workspace, title, rest, key)?,
        ["next", workspace, pane, generation] => next(workspace, pane, generation, key)?,
        ["show", workspace, task] => show(workspace, task),
        ["list", workspace, rest @ ..] => list(workspace, rest)?,
        ["update", workspace, task, version, rest @ ..] => {
            update(workspace, task, version, rest, key)?
        }
        ["comment", workspace, task, text, rest @ ..] => comment(workspace, task, text, rest, key)?,
        ["subitem", "add", workspace, task, text, rest @ ..] => {
            subitem_add(workspace, task, text, rest, key)?
        }
        ["subitem", "done", workspace, task, subitem, rest @ ..] => {
            subitem_done(workspace, task, subitem, rest, key)?
        }
        ["block", workspace, task, kind, reason, rest @ ..] => {
            block(workspace, task, kind, reason, rest, key)?
        }
        ["release", workspace, task, rest @ ..] => release(workspace, task, rest, key)?,
        ["request-review", workspace, task, rest @ ..] => {
            request_review(workspace, task, rest, key)?
        }
        ["complete", workspace, task, summary, rest @ ..] => {
            complete(workspace, task, summary, rest, key)?
        }
        ["dispatch", workspace, task, rest @ ..] => dispatch(workspace, task, rest, key)?,
        ["unblock", workspace, task, rest @ ..] => unblock(workspace, task, rest, key)?,
        ["move", workspace, task, state, reason] => move_task(workspace, task, state, reason, key),
        ["worktree-clean", workspace, task] => worktree_clean(workspace, task, key),
        _ => return Err(invalid(crate::commands::USAGE)),
    };
    Ok(request)
}

pub fn execute_loop(client: &mut Client, args: &[&str], key: &str) -> Result<Value> {
    parse_loop(args, key)?.run(client)
}

fn parse_loop(args: &[&str], key: &str) -> Result<Request> {
    let ["set", pane, prompt, interval, rounds, stop_when, enabled] = args else {
        return Err(invalid(crate::commands::USAGE));
    };
    let params = json!({
        "paneId":pane,
        "prompt":prompt,
        "interval":number(interval, "interval")?,
        "maxRounds":number(rounds, "max rounds")?,
        "stopWhen":one_of(stop_when, "stop condition", &["round_limit","three_empty","explicit_off"] )?,
        "enabled":boolean(enabled, "enabled")?,
        "idempotencyKey":key,
    });
    Ok(Request::LoopSet(params))
}

fn create(workspace: &str, title: &str, args: &[&str], key: &str) -> Result<Request> {
    let flags = Flags::parse(
        args,
        &[
            "--description",
            "--how-to-verify",
            "--must-not-change",
            "--harness",
            "--model",
            "--effort",
            "--priority",
            "--create-worktree",
            "--permission-mode",
            "--dependencies",
        ],
    )?;
    let mut params = Map::new();
    insert(&mut params, "workspaceId", workspace);
    insert(&mut params, "title", title);
    insert(
        &mut params,
        "mustNotChange",
        flags.required("--must-not-change")?,
    );
    optional(&mut params, "description", flags.get("--description"));
    optional(&mut params, "howToVerify", flags.get("--how-to-verify"));
    if let Some(priority) = flags.get("--priority") {
        insert(
            &mut params,
            "priority",
            one_of(priority, "priority", &["urgent", "high", "medium", "low"])?,
        );
    }
    if let Some(value) = flags.boolean("--create-worktree")? {
        params.insert("createWorktree".into(), json!(value));
    }
    optional(
        &mut params,
        "permissionMode",
        flags.get("--permission-mode"),
    );
    if let Some(value) = flags.get("--dependencies") {
        params.insert("dependencies".into(), json!(csv(value)));
    }
    if let Some(executor) = executor(&flags, false)? {
        params.insert("executor".into(), executor);
    }
    insert(&mut params, "idempotencyKey", key);
    Ok(Request::Create(Value::Object(params)))
}

fn next(workspace: &str, pane: &str, generation: &str, key: &str) -> Result<Request> {
    Ok(Request::Next(json!({
        "workspaceId":workspace,
        "paneId":pane,
        "sessionGeneration":number(generation, "session generation")?,
        "idempotencyKey":key,
    })))
}

fn show(workspace: &str, task: &str) -> Request {
    Request::Show(json!({"workspaceId":workspace,"taskId":task}))
}

fn list(workspace: &str, args: &[&str]) -> Result<Request> {
    let flags = Flags::parse(args, &["--state", "--limit", "--cursor"])?;
    let limit = flags.number("--limit")?.unwrap_or(100);
    if !(1..=200).contains(&limit) {
        return Err(invalid("Task list limit must be between 1 and 200"));
    }
    let mut params = json!({"workspaceId":workspace,"limit":limit});
    if let Some(state) = flags.get("--state") {
        params["state"] = json!(one_of(
            state,
            "task state",
            &["todo", "in_progress", "in_review", "completed", "cancelled"],
        )?);
    }
    if let Some(cursor) = flags.get("--cursor") {
        params["cursor"] = json!(cursor);
    }
    Ok(Request::List(params))
}

fn update(workspace: &str, task: &str, version: &str, args: &[&str], key: &str) -> Result<Request> {
    let flags = Flags::parse(
        args,
        &[
            "--title",
            "--description",
            "--how-to-verify",
            "--must-not-change",
            "--priority",
            "--dependencies",
            "--planned-owner",
            "--pane",
            "--generation",
            "--attempt",
        ],
    )?;
    let mut params = base(workspace, task, key);
    params.insert("version".into(), json!(number(version, "version")?));
    for (flag, field) in [
        ("--title", "title"),
        ("--description", "description"),
        ("--how-to-verify", "howToVerify"),
        ("--must-not-change", "mustNotChange"),
        ("--planned-owner", "plannedOwner"),
    ] {
        optional(&mut params, field, flags.get(flag));
    }
    if let Some(priority) = flags.get("--priority") {
        insert(
            &mut params,
            "priority",
            one_of(priority, "priority", &["urgent", "high", "medium", "low"])?,
        );
    }
    if let Some(value) = flags.get("--dependencies") {
        params.insert("dependencies".into(), json!(csv(value)));
    }
    executor_identity(&flags, &mut params)?;
    Ok(Request::Update(Value::Object(params)))
}

fn comment(workspace: &str, task: &str, text: &str, args: &[&str], key: &str) -> Result<Request> {
    let flags = executor_flags(args)?;
    let mut params = base(workspace, task, key);
    insert(&mut params, "text", text);
    executor_identity(&flags, &mut params)?;
    Ok(Request::Comment(Value::Object(params)))
}

fn subitem_add(
    workspace: &str,
    task: &str,
    text: &str,
    args: &[&str],
    key: &str,
) -> Result<Request> {
    let flags = Flags::parse(
        args,
        &[
            "--agent",
            "--instructions",
            "--pane",
            "--generation",
            "--attempt",
        ],
    )?;
    let mut params = base(workspace, task, key);
    insert(&mut params, "text", text);
    optional(&mut params, "agent", flags.get("--agent"));
    optional(&mut params, "instructions", flags.get("--instructions"));
    executor_identity(&flags, &mut params)?;
    Ok(Request::SubitemAdd(Value::Object(params)))
}

fn subitem_done(
    workspace: &str,
    task: &str,
    subitem: &str,
    args: &[&str],
    key: &str,
) -> Result<Request> {
    let flags = executor_flags(args)?;
    let mut params = base(workspace, task, key);
    insert(&mut params, "subitemId", subitem);
    executor_identity(&flags, &mut params)?;
    Ok(Request::SubitemDone(Value::Object(params)))
}

fn block(
    workspace: &str,
    task: &str,
    kind: &str,
    reason: &str,
    args: &[&str],
    key: &str,
) -> Result<Request> {
    let flags = Flags::parse(
        args,
        &["--dependency", "--pane", "--generation", "--attempt"],
    )?;
    let mut params = base(workspace, task, key);
    insert(
        &mut params,
        "kind",
        one_of(
            kind,
            "blocking kind",
            &["dependency", "missing_information", "external_error"],
        )?,
    );
    insert(&mut params, "reason", reason);
    optional(&mut params, "dependencyTaskId", flags.get("--dependency"));
    executor_identity(&flags, &mut params)?;
    Ok(Request::Block(Value::Object(params)))
}

fn release(workspace: &str, task: &str, args: &[&str], key: &str) -> Result<Request> {
    let flags = Flags::parse(args, &["--reason", "--pane", "--generation", "--attempt"])?;
    let mut params = base(workspace, task, key);
    optional(&mut params, "reason", flags.get("--reason"));
    executor_identity(&flags, &mut params)?;
    Ok(Request::Release(Value::Object(params)))
}

fn request_review(workspace: &str, task: &str, args: &[&str], key: &str) -> Result<Request> {
    let flags = Flags::parse(args, &["--reason", "--pane", "--generation", "--attempt"])?;
    let mut params = base(workspace, task, key);
    optional(&mut params, "reason", flags.get("--reason"));
    executor_identity(&flags, &mut params)?;
    Ok(Request::Review(Value::Object(params)))
}

fn complete(
    workspace: &str,
    task: &str,
    summary: &str,
    args: &[&str],
    key: &str,
) -> Result<Request> {
    let flags = Flags::parse(
        args,
        &[
            "--contract-changes",
            "--out-of-scope-edits",
            "--pane",
            "--generation",
            "--attempt",
        ],
    )?;
    let mut params = base(workspace, task, key);
    insert(&mut params, "summary", summary);
    params.insert(
        "contractChanges".into(),
        json_array(
            flags.get("--contract-changes").unwrap_or("[]"),
            "contract changes",
        )?,
    );
    params.insert(
        "outOfScopeEdits".into(),
        json_array(
            flags.get("--out-of-scope-edits").unwrap_or("[]"),
            "out-of-scope edits",
        )?,
    );
    executor_identity(&flags, &mut params)?;
    Ok(Request::Complete(Value::Object(params)))
}

fn dispatch(workspace: &str, task: &str, args: &[&str], key: &str) -> Result<Request> {
    let flags = Flags::parse(
        args,
        &[
            "--workspace-default",
            "--harness",
            "--model",
            "--effort",
            "--permission-mode",
        ],
    )?;
    let preset = executor(&flags, true)?.ok_or_else(|| invalid("Executor preset is required"))?;
    Ok(Request::Dispatch(json!({
        "workspaceId":workspace,"taskId":task,"preset":preset,"idempotencyKey":key
    })))
}

fn unblock(workspace: &str, task: &str, args: &[&str], key: &str) -> Result<Request> {
    let flags = Flags::parse(args, &["--reason"])?;
    let mut params = base(workspace, task, key);
    optional(&mut params, "reason", flags.get("--reason"));
    Ok(Request::Unblock(Value::Object(params)))
}

fn move_task(workspace: &str, task: &str, state: &str, reason: &str, key: &str) -> Request {
    Request::Move(json!({
        "workspaceId":workspace,"taskId":task,"state":state,"reason":reason,"idempotencyKey":key
    }))
}

fn worktree_clean(workspace: &str, task: &str, key: &str) -> Request {
    Request::WorktreeClean(json!({
        "workspaceId":workspace,"taskId":task,"idempotencyKey":key
    }))
}

fn executor(flags: &Flags, required: bool) -> Result<Option<Value>> {
    let selected = [
        "--workspace-default",
        "--harness",
        "--model",
        "--effort",
        "--permission-mode",
    ]
    .iter()
    .any(|name| flags.get(name).is_some());
    if !selected && !required {
        return Ok(None);
    }
    let workspace_default = flags
        .boolean("--workspace-default")?
        .unwrap_or_else(|| flags.get("--harness").is_none());
    let mut value = Map::new();
    value.insert("workspaceDefault".into(), json!(workspace_default));
    for (flag, field) in [
        ("--harness", "harness"),
        ("--model", "model"),
        ("--effort", "effort"),
        ("--permission-mode", "permissionMode"),
    ] {
        optional(&mut value, field, flags.get(flag));
    }
    Ok(Some(Value::Object(value)))
}

fn executor_flags(args: &[&str]) -> Result<Flags> {
    Flags::parse(args, &["--pane", "--generation", "--attempt"])
}

fn executor_identity(flags: &Flags, params: &mut Map<String, Value>) -> Result<()> {
    let pane = flags.get("--pane");
    let generation = flags.number("--generation")?;
    let attempt = flags.number("--attempt")?;
    let supplied = usize::from(pane.is_some())
        + usize::from(generation.is_some())
        + usize::from(attempt.is_some());
    if supplied != 0 && supplied != 3 {
        return Err(invalid(
            "Executor identity requires --pane, --generation and --attempt together",
        ));
    }
    if let (Some(pane), Some(generation), Some(attempt)) = (pane, generation, attempt) {
        insert(params, "paneId", pane);
        params.insert("sessionGeneration".into(), json!(generation));
        params.insert("attempt".into(), json!(attempt));
    }
    Ok(())
}

fn base(workspace: &str, task: &str, key: &str) -> Map<String, Value> {
    let mut params = Map::new();
    insert(&mut params, "workspaceId", workspace);
    insert(&mut params, "taskId", task);
    insert(&mut params, "idempotencyKey", key);
    params
}

fn insert(params: &mut Map<String, Value>, field: &str, value: &str) {
    params.insert(field.into(), json!(value));
}

fn optional(params: &mut Map<String, Value>, field: &str, value: Option<&str>) {
    if let Some(value) = value {
        insert(params, field, value);
    }
}

fn number(value: &str, name: &str) -> Result<u32> {
    value
        .parse()
        .map_err(|_| invalid(format!("{name} must be an integer")))
}

fn boolean(value: &str, name: &str) -> Result<bool> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid(format!("{name} must be true or false"))),
    }
}

fn one_of<'a>(value: &'a str, name: &str, allowed: &[&str]) -> Result<&'a str> {
    if allowed.contains(&value) {
        Ok(value)
    } else {
        Err(invalid(format!("Invalid {name}: {value}")))
    }
}

fn csv(value: &str) -> Vec<&str> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect()
}

fn json_array(value: &str, name: &str) -> Result<Value> {
    let parsed: Value =
        serde_json::from_str(value).map_err(|_| invalid(format!("{name} must be a JSON array")))?;
    if !parsed.is_array() {
        return Err(invalid(format!("{name} must be a JSON array")));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
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
}
