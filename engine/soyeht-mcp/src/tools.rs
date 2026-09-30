use crate::{Error, Result, engine::EnginePort};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Map, Value, json};
use soyeht_protocol::Role;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskContext {
    pub workspace_id: String,
    pub pane_id: String,
    pub session_generation: u32,
    pub task_id: Option<String>,
    pub attempt: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Audience {
    Agent,
    Chief,
    Both,
}

struct Tool {
    name: &'static str,
    method: &'static str,
    audience: Audience,
    description: &'static str,
    schema: fn() -> Value,
}

const TOOLS: [Tool; 35] = [
    Tool {
        name: "list_agents",
        method: "agent.list",
        audience: Audience::Both,
        description: "List other ACP agent panes in this workspace, including IDs for messaging.",
        schema: empty_schema,
    },
    Tool {
        name: "message_agent",
        method: "agent.message",
        audience: Audience::Both,
        description: "Queue a message for another ACP agent; delivery waits for its clean input.",
        schema: agent_message_schema,
    },
    Tool {
        name: "task_create",
        method: "task.create",
        audience: Audience::Chief,
        description: "Create a workspace task with explicit preservation constraints.",
        schema: create_schema,
    },
    Tool {
        name: "task_next",
        method: "task.next",
        audience: Audience::Agent,
        description: "Claim the next eligible task for this pane.",
        schema: empty_schema,
    },
    Tool {
        name: "task_show",
        method: "task.show",
        audience: Audience::Both,
        description: "Show the current task or a selected workspace task.",
        schema: task_selector_schema,
    },
    Tool {
        name: "task_list",
        method: "task.list",
        audience: Audience::Chief,
        description: "List workspace tasks.",
        schema: list_schema,
    },
    Tool {
        name: "task_update",
        method: "task.update",
        audience: Audience::Both,
        description: "Update task fields using its current version.",
        schema: update_schema,
    },
    Tool {
        name: "task_comment",
        method: "task.comment",
        audience: Audience::Both,
        description: "Add a durable task comment.",
        schema: comment_schema,
    },
    Tool {
        name: "task_subitem_add",
        method: "task.subitem.add",
        audience: Audience::Agent,
        description: "Add a task subitem before executing it.",
        schema: subitem_add_schema,
    },
    Tool {
        name: "task_subitem_done",
        method: "task.subitem.done",
        audience: Audience::Agent,
        description: "Mark one task subitem as done.",
        schema: subitem_done_schema,
    },
    Tool {
        name: "task_block",
        method: "task.block",
        audience: Audience::Agent,
        description: "Block the task with a typed reason.",
        schema: block_schema,
    },
    Tool {
        name: "task_release",
        method: "task.release",
        audience: Audience::Agent,
        description: "Return the current task to the workspace list.",
        schema: release_schema,
    },
    Tool {
        name: "task_request_review",
        method: "task.request_review",
        audience: Audience::Agent,
        description: "Request review after all subitems are done.",
        schema: release_schema,
    },
    Tool {
        name: "task_complete",
        method: "task.complete",
        audience: Audience::Agent,
        description: "Complete the task with explicit contract and scope declarations.",
        schema: complete_schema,
    },
    Tool {
        name: "task_dispatch",
        method: "task.dispatch",
        audience: Audience::Chief,
        description: "Dispatch a task with an executor preset.",
        schema: dispatch_schema,
    },
    Tool {
        name: "task_unblock",
        method: "task.unblock",
        audience: Audience::Chief,
        description: "Remove a task block with a recorded reason.",
        schema: unblock_schema,
    },
    Tool {
        name: "loop_set",
        method: "loop.set",
        audience: Audience::Both,
        description: "Configure the loop for this pane.",
        schema: loop_schema,
    },
    Tool {
        name: "writing_structure",
        method: "writing.structure",
        audience: Audience::Both,
        description: "Read the bounded structure of an open writing document.",
        schema: writing_structure_schema,
    },
    Tool {
        name: "writing_variant_add",
        method: "writing.variant.add",
        audience: Audience::Both,
        description: "Add one alternate take to a screenplay line for the writer to choose.",
        schema: writing_variant_add_schema,
    },
    Tool {
        name: "web_open",
        method: "web.open",
        audience: Audience::Both,
        description: "Open or focus a web pane in the bound workspace.",
        schema: web_open_schema,
    },
    Tool {
        name: "web_navigate",
        method: "web.navigate",
        audience: Audience::Both,
        description: "Navigate a web pane to a URL or through its local history.",
        schema: web_navigate_schema,
    },
    Tool {
        name: "web_snapshot",
        method: "web.snapshot",
        audience: Audience::Both,
        description: "Read a bounded accessibility snapshot from a visible web pane.",
        schema: web_snapshot_schema,
    },
    Tool {
        name: "web_click",
        method: "web.click",
        audience: Audience::Both,
        description: "Click an element reference from the latest web snapshot.",
        schema: web_ref_schema,
    },
    Tool {
        name: "web_type",
        method: "web.type",
        audience: Audience::Both,
        description: "Type bounded text into an element reference.",
        schema: web_type_schema,
    },
    Tool {
        name: "web_press",
        method: "web.press",
        audience: Audience::Both,
        description: "Send one bounded key name to a web pane.",
        schema: web_press_schema,
    },
    Tool {
        name: "web_hover",
        method: "web.hover",
        audience: Audience::Both,
        description: "Hover an element reference from the latest web snapshot.",
        schema: web_ref_schema,
    },
    Tool {
        name: "web_select",
        method: "web.select",
        audience: Audience::Both,
        description: "Select a bounded value in a referenced web control.",
        schema: web_select_schema,
    },
    Tool {
        name: "web_scroll",
        method: "web.scroll",
        audience: Audience::Both,
        description: "Scroll a web pane or a referenced element by bounded deltas.",
        schema: web_scroll_schema,
    },
    Tool {
        name: "web_wait",
        method: "web.wait",
        audience: Audience::Both,
        description: "Wait for bounded text, selector, or network-idle conditions.",
        schema: web_wait_schema,
    },
    Tool {
        name: "web_evaluate",
        method: "web.evaluate",
        audience: Audience::Both,
        description: "Evaluate bounded page code after a separate human permission.",
        schema: web_evaluate_schema,
    },
    Tool {
        name: "web_screenshot",
        method: "web.screenshot",
        audience: Audience::Both,
        description: "Capture a bounded screenshot from a visible web pane.",
        schema: web_screenshot_schema,
    },
    Tool {
        name: "web_console",
        method: "web.console",
        audience: Audience::Both,
        description: "Read bounded console metadata without page or transport bodies.",
        schema: web_since_schema,
    },
    Tool {
        name: "web_network",
        method: "web.network",
        audience: Audience::Both,
        description: "Read bounded network metadata without headers or bodies.",
        schema: web_since_schema,
    },
    Tool {
        name: "web_resize",
        method: "web.resize",
        audience: Audience::Both,
        description: "Resize a visible web pane using a preset or explicit dimensions.",
        schema: web_resize_schema,
    },
    Tool {
        name: "web_close",
        method: "web.close",
        audience: Audience::Both,
        description: "Close a visible web pane.",
        schema: web_pane_schema,
    },
];

pub struct Service<P> {
    engine: P,
    context: TaskContext,
}

impl<P: EnginePort> Service<P> {
    pub fn new(engine: P, context: TaskContext) -> Self {
        Self { engine, context }
    }

    pub fn list(&self) -> Value {
        let tools = TOOLS
            .iter()
            .filter(|tool| allowed(self.engine.role(), tool.audience))
            .map(|tool| json!({"name":tool.name,"description":tool.description,"inputSchema":schema_for(tool, self.engine.role())}))
            .collect::<Vec<_>>();
        json!({"tools":tools})
    }

    pub fn call(&mut self, name: &str, arguments: Value) -> Result<Value> {
        let tool = TOOLS
            .iter()
            .find(|tool| tool.name == name)
            .ok_or(Error::UnknownTool)?;
        if !allowed(self.engine.role(), tool.audience) {
            return Err(Error::ToolForbidden);
        }
        let params = self.parameters(tool, arguments)?;
        let response = self.engine.request_with_attachment(tool.method, params)?;
        let text = serde_json::to_string(&response.value)?;
        let mut content = vec![json!({"type":"text","text":text})];
        if let Some(attachment) = response.attachment {
            content.push(json!({
                "type":"image",
                "data":STANDARD.encode(attachment.bytes),
                "mimeType":attachment.reference.mime_type
            }));
        }
        Ok(json!({
            "content":content,
            "structuredContent":response.value,
            "isError":false
        }))
    }

    fn parameters(&self, tool: &Tool, arguments: Value) -> Result<Value> {
        let mut params = arguments
            .as_object()
            .cloned()
            .ok_or(Error::InvalidArguments("tool arguments must be an object"))?;
        let schema = (tool.schema)();
        let properties = schema["properties"]
            .as_object()
            .ok_or(Error::InvalidArguments(
                "tool schema must declare properties",
            ))?;
        if params.keys().any(|name| !properties.contains_key(name)) {
            return Err(Error::InvalidArguments("unknown tool argument"));
        }
        if tool.method.starts_with("task.") {
            params.insert("workspaceId".into(), json!(self.context.workspace_id));
        }
        if tool.name == "web_open" {
            params.insert("workspaceId".into(), json!(self.context.workspace_id));
        }
        if mutating(tool.method) {
            params.insert("idempotencyKey".into(), json!(Uuid::new_v4().to_string()));
        }
        if tool.name == "loop_set" || tool.name == "task_next" {
            params.insert("paneId".into(), json!(self.context.pane_id));
        }
        if tool.name == "task_next" {
            params.insert(
                "sessionGeneration".into(),
                json!(self.context.session_generation),
            );
        }
        if tool.name == "task_block"
            && let Some(kind) = params.remove("type")
        {
            params.insert("kind".into(), kind);
        }
        if self.engine.role() == &Role::Agent && requires_task(tool.name) {
            let (task_id, attempt) = self
                .context
                .task_id
                .as_ref()
                .zip(self.context.attempt)
                .ok_or(Error::TaskContextRequired)?;
            params.insert("taskId".into(), json!(task_id));
            params.insert("paneId".into(), json!(self.context.pane_id));
            params.insert(
                "sessionGeneration".into(),
                json!(self.context.session_generation),
            );
            params.insert("attempt".into(), json!(attempt));
        }
        let value = Value::Object(params);
        soyeht_protocol::validate::parameters(tool.method, &value)?;
        Ok(value)
    }
}

fn schema_for(tool: &Tool, role: &Role) -> Value {
    let mut schema = (tool.schema)();
    if role == &Role::Agent
        && requires_task(tool.name)
        && let Some(properties) = schema["properties"].as_object_mut()
    {
        properties.remove("taskId");
    }
    if role == &Role::Chief
        && matches!(tool.name, "task_show" | "task_update" | "task_comment")
        && let Some(required) = schema["required"].as_array_mut()
    {
        required.push(json!("taskId"));
    }
    schema
}

fn allowed(role: &Role, audience: Audience) -> bool {
    matches!(
        (role, audience),
        (Role::Agent, Audience::Agent | Audience::Both)
            | (Role::Chief, Audience::Chief | Audience::Both)
    )
}

fn requires_task(name: &str) -> bool {
    matches!(
        name,
        "task_show"
            | "task_update"
            | "task_comment"
            | "task_subitem_add"
            | "task_subitem_done"
            | "task_block"
            | "task_release"
            | "task_request_review"
            | "task_complete"
    )
}

fn mutating(method: &str) -> bool {
    !matches!(
        method,
        "task.show"
            | "agent.list"
            | "task.list"
            | "writing.structure"
            | "web.snapshot"
            | "web.wait"
            | "web.screenshot"
            | "web.console"
            | "web.network"
    )
}

fn agent_message_schema() -> Value {
    object(
        fields(&[
            ("targetPaneId", id()),
            (
                "text",
                bounded_string(soyeht_protocol::limits::AGENT_MESSAGE_BYTES),
            ),
        ]),
        &["targetPaneId", "text"],
    )
}

fn object(properties: Map<String, Value>, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

fn fields(entries: &[(&str, Value)]) -> Map<String, Value> {
    entries
        .iter()
        .map(|(name, value)| ((*name).into(), value.clone()))
        .collect()
}

fn string() -> Value {
    json!({"type":"string"})
}
fn id() -> Value {
    json!({"type":"string","minLength":1,"maxLength":128})
}
fn bounded_string(max: usize) -> Value {
    json!({"type":"string","maxLength":max})
}
fn boolean() -> Value {
    json!({"type":"boolean"})
}
fn integer(minimum: i64, maximum: i64) -> Value {
    json!({"type":"integer","minimum":minimum,"maximum":maximum})
}
fn decimal() -> Value {
    json!({"type":"string","pattern":"^(0|[1-9][0-9]*)$","maxLength":20})
}
fn strings() -> Value {
    json!({"type":"array","items":{"type":"string"}})
}
fn empty_schema() -> Value {
    object(Map::new(), &[])
}
fn task_selector_schema() -> Value {
    object(fields(&[("taskId", id())]), &[])
}
fn release_schema() -> Value {
    object(fields(&[("reason", string())]), &[])
}
fn comment_schema() -> Value {
    object(fields(&[("taskId", id()), ("text", string())]), &["text"])
}
fn subitem_add_schema() -> Value {
    object(
        fields(&[
            ("text", string()),
            ("agent", id()),
            ("instructions", string()),
        ]),
        &["text"],
    )
}
fn subitem_done_schema() -> Value {
    object(fields(&[("subitemId", id())]), &["subitemId"])
}
fn unblock_schema() -> Value {
    object(
        fields(&[("taskId", id()), ("reason", string())]),
        &["taskId"],
    )
}

fn block_schema() -> Value {
    object(
        fields(&[
            (
                "type",
                json!({"type":"string","enum":["dependency","missing_information","external_error"]}),
            ),
            ("reason", string()),
            ("dependencyTaskId", id()),
        ]),
        &["type", "reason"],
    )
}

fn create_schema() -> Value {
    object(
        fields(&[
            ("title", string()),
            ("description", string()),
            ("howToVerify", string()),
            ("mustNotChange", string()),
            (
                "executor",
                json!({"type":"object","required":["workspaceDefault"]}),
            ),
            (
                "priority",
                json!({"type":"string","enum":["urgent","high","medium","low"]}),
            ),
            ("createWorktree", json!({"type":"boolean"})),
            ("permissionMode", id()),
            ("dependencies", strings()),
            ("dispatch", json!({"type":"boolean"})),
        ]),
        &["title", "mustNotChange"],
    )
}

fn list_schema() -> Value {
    object(
        fields(&[
            (
                "state",
                json!({"type":"string","enum":["todo","in_progress","in_review","completed","cancelled"]}),
            ),
            ("limit", json!({"type":"integer","minimum":1,"maximum":200})),
            ("cursor", id()),
        ]),
        &["limit"],
    )
}

fn update_schema() -> Value {
    object(
        fields(&[
            ("taskId", id()),
            ("version", json!({"type":"integer","minimum":0})),
            ("title", string()),
            ("description", string()),
            (
                "priority",
                json!({"type":"string","enum":["urgent","high","medium","low"]}),
            ),
            ("dependencies", strings()),
        ]),
        &["version"],
    )
}

fn complete_schema() -> Value {
    object(
        fields(&[
            ("summary", string()),
            (
                "contractChanges",
                json!({"type":"array","items":{"type":"object","required":["path","name","label","contractRef"]}}),
            ),
            (
                "outOfScopeEdits",
                json!({"type":"array","items":{"type":"object","required":["path","reason"]}}),
            ),
        ]),
        &["summary", "contractChanges", "outOfScopeEdits"],
    )
}

fn dispatch_schema() -> Value {
    object(
        fields(&[
            ("taskId", id()),
            (
                "preset",
                json!({"type":"object","required":["workspaceDefault"]}),
            ),
        ]),
        &["taskId", "preset"],
    )
}

fn loop_schema() -> Value {
    object(
        fields(&[
            ("prompt", string()),
            ("interval", json!({"type":"integer","minimum":0})),
            ("maxRounds", json!({"type":"integer","minimum":0})),
            (
                "stopWhen",
                json!({"type":"string","enum":["round_limit","three_empty","manual"]}),
            ),
            ("enabled", json!({"type":"boolean"})),
        ]),
        &["prompt", "interval", "maxRounds", "stopWhen", "enabled"],
    )
}

fn writing_structure_schema() -> Value {
    object(
        fields(&[("fileId", id()), ("revision", decimal())]),
        &["fileId", "revision"],
    )
}

fn writing_variant_add_schema() -> Value {
    object(
        fields(&[
            ("fileId", id()),
            ("lineId", id()),
            ("baseRevision", decimal()),
            (
                "text",
                bounded_string(soyeht_protocol::limits::WRITING_VARIANT_BYTES),
            ),
        ]),
        &["fileId", "lineId", "baseRevision", "text"],
    )
}

fn web_open_schema() -> Value {
    object(
        fields(&[
            (
                "url",
                json!({"type":"string","minLength":1,"maxLength":4096}),
            ),
            ("workspaceId", id()),
            ("newPane", boolean()),
            ("incognito", boolean()),
            ("mode", json!({"type":"string","enum":["structured"]})),
        ]),
        &["url"],
    )
}

fn web_navigate_schema() -> Value {
    object(
        fields(&[
            ("paneId", id()),
            (
                "url",
                json!({"type":"string","minLength":1,"maxLength":4096}),
            ),
            (
                "action",
                json!({"type":"string","enum":["back","forward","reload"]}),
            ),
        ]),
        &["paneId"],
    )
}

fn web_pane_schema() -> Value {
    object(fields(&[("paneId", id())]), &["paneId"])
}

fn web_snapshot_schema() -> Value {
    object(
        fields(&[("paneId", id()), ("screenshot", boolean())]),
        &["paneId"],
    )
}

fn web_ref_schema() -> Value {
    object(
        fields(&[("paneId", id()), ("ref", id())]),
        &["paneId", "ref"],
    )
}

fn web_type_schema() -> Value {
    object(
        fields(&[
            ("paneId", id()),
            ("ref", id()),
            ("text", bounded_string(65_536)),
            ("submit", boolean()),
        ]),
        &["paneId", "ref", "text"],
    )
}

fn web_press_schema() -> Value {
    object(
        fields(&[
            ("paneId", id()),
            (
                "key",
                json!({"type":"string","minLength":1,"maxLength":128}),
            ),
        ]),
        &["paneId", "key"],
    )
}

fn web_select_schema() -> Value {
    object(
        fields(&[
            ("paneId", id()),
            ("ref", id()),
            ("value", bounded_string(4096)),
        ]),
        &["paneId", "ref", "value"],
    )
}

fn web_scroll_schema() -> Value {
    object(
        fields(&[
            ("paneId", id()),
            ("ref", id()),
            ("dx", integer(-1_000_000, 1_000_000)),
            ("dy", integer(-1_000_000, 1_000_000)),
        ]),
        &["paneId", "dx", "dy"],
    )
}

fn web_wait_schema() -> Value {
    object(
        fields(&[
            ("paneId", id()),
            ("text", bounded_string(4096)),
            ("selector", bounded_string(4096)),
            ("networkIdle", boolean()),
            ("timeoutMs", integer(0, 60_000)),
        ]),
        &["paneId", "timeoutMs"],
    )
}

fn web_evaluate_schema() -> Value {
    object(
        fields(&[("paneId", id()), ("js", bounded_string(65_536))]),
        &["paneId", "js"],
    )
}

fn web_screenshot_schema() -> Value {
    object(
        fields(&[("paneId", id()), ("fullPage", boolean())]),
        &["paneId"],
    )
}

fn web_since_schema() -> Value {
    object(
        fields(&[("paneId", id()), ("since", decimal())]),
        &["paneId"],
    )
}

fn web_resize_schema() -> Value {
    object(
        fields(&[
            ("paneId", id()),
            (
                "preset",
                json!({"type":"string","enum":["iphone_se","iphone_15_pro","ipad","responsive"]}),
            ),
            ("width", integer(1, 16_384)),
            ("height", integer(1, 16_384)),
            ("rotate", boolean()),
        ]),
        &["paneId"],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{EngineAttachment, EngineResponse};
    use soyeht_protocol::AttachmentRef;
    use std::collections::VecDeque;

    struct FakeEngine {
        role: Role,
        calls: VecDeque<(String, Value)>,
    }
    impl EnginePort for FakeEngine {
        fn role(&self) -> &Role {
            &self.role
        }
        fn request(&mut self, method: &str, params: Value) -> Result<Value> {
            self.calls.push_back((method.into(), params));
            Ok(json!({"accepted":true}))
        }
    }
    fn context(task: bool) -> TaskContext {
        TaskContext {
            workspace_id: "workspace".into(),
            pane_id: "pane".into(),
            session_generation: 7,
            task_id: task.then(|| "WS-1".into()),
            attempt: task.then_some(3),
        }
    }

    #[test]
    fn agent_tools_override_spoofed_identity_and_require_a_current_attempt() {
        let engine = FakeEngine {
            role: Role::Agent,
            calls: VecDeque::new(),
        };
        let mut service = Service::new(engine, context(true));
        service
            .call("task_comment", json!({"text":"milestone","taskId":"other"}))
            .unwrap();
        let (method, params) = service.engine.calls.pop_front().unwrap();
        assert_eq!(method, "task.comment");
        assert_eq!(params["workspaceId"], "workspace");
        assert_eq!(params["taskId"], "WS-1");
        assert_eq!(params["paneId"], "pane");
        assert_eq!(params["sessionGeneration"], 7);
        assert_eq!(params["attempt"], 3);
        assert!(params["idempotencyKey"].as_str().is_some());
        assert!(matches!(
            service.call("task_comment", json!({"text":"x","attempt":99})),
            Err(Error::InvalidArguments("unknown tool argument"))
        ));

        let engine = FakeEngine {
            role: Role::Agent,
            calls: VecDeque::new(),
        };
        let mut service = Service::new(engine, context(false));
        assert!(matches!(
            service.call(
                "task_complete",
                json!({"summary":"done","contractChanges":[],"outOfScopeEdits":[]})
            ),
            Err(Error::TaskContextRequired)
        ));
    }

    #[test]
    fn peer_tools_use_binding_identity_and_validate_target() {
        let engine = FakeEngine {
            role: Role::Agent,
            calls: VecDeque::new(),
        };
        let mut service = Service::new(engine, context(false));
        let listing = service.list();
        let names = listing["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"list_agents"));
        assert!(names.contains(&"message_agent"));
        service.call("list_agents", json!({})).unwrap();
        assert_eq!(service.engine.calls.pop_front().unwrap().0, "agent.list");
        service
            .call(
                "message_agent",
                json!({"targetPaneId":"recipient","text":"hello"}),
            )
            .unwrap();
        let (method, params) = service.engine.calls.pop_front().unwrap();
        assert_eq!(method, "agent.message");
        assert_eq!(params["targetPaneId"], "recipient");
        assert!(params["idempotencyKey"].as_str().is_some());
        assert!(params.get("paneId").is_none());
        assert!(
            service
                .call(
                    "message_agent",
                    json!({"targetPaneId":"recipient","text":"hello","paneId":"spoof"})
                )
                .is_err()
        );
    }

    #[test]
    fn role_lists_are_closed_and_the_lead_cannot_complete() {
        let agent = Service::new(
            FakeEngine {
                role: Role::Agent,
                calls: VecDeque::new(),
            },
            context(false),
        );
        let chief = Service::new(
            FakeEngine {
                role: Role::Chief,
                calls: VecDeque::new(),
            },
            context(false),
        );
        let names = |value: Value| {
            value["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tool| tool["name"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        let agent_names = names(agent.list());
        let chief_names = names(chief.list());
        assert!(
            TOOLS
                .iter()
                .all(|tool| tool.method != "pane.experiment.hook")
        );
        assert!(agent_names.contains(&"task_next".into()));
        assert!(agent_names.contains(&"task_complete".into()));
        assert!(!agent_names.contains(&"task_dispatch".into()));
        assert!(chief_names.contains(&"task_dispatch".into()));
        assert!(chief_names.contains(&"task_list".into()));
        assert!(!chief_names.contains(&"task_complete".into()));
        for name in ["writing_structure", "writing_variant_add"] {
            assert!(agent_names.contains(&name.into()));
            assert!(chief_names.contains(&name.into()));
        }
        for name in [
            "web_open",
            "web_navigate",
            "web_snapshot",
            "web_click",
            "web_type",
            "web_press",
            "web_hover",
            "web_select",
            "web_scroll",
            "web_wait",
            "web_evaluate",
            "web_screenshot",
            "web_console",
            "web_network",
            "web_resize",
            "web_close",
        ] {
            assert!(agent_names.contains(&name.into()));
            assert!(chief_names.contains(&name.into()));
        }
        let agent_tools = agent.list();
        let comment = agent_tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "task_comment")
            .unwrap();
        assert!(comment["inputSchema"]["properties"].get("taskId").is_none());
        let chief_tools = chief.list();
        let update = chief_tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "task_update")
            .unwrap();
        assert!(
            update["inputSchema"]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("taskId"))
        );
    }

    #[test]
    fn every_advertised_tool_builds_protocol_valid_parameters() {
        let mut agent = Service::new(
            FakeEngine {
                role: Role::Agent,
                calls: VecDeque::new(),
            },
            context(true),
        );
        for (name, arguments) in [
            ("task_next", json!({})),
            ("task_show", json!({})),
            ("task_update", json!({"version":1,"title":"Revised"})),
            ("task_comment", json!({"text":"Milestone"})),
            ("task_subitem_add", json!({"text":"Plan"})),
            ("task_subitem_done", json!({"subitemId":"WS-1-T01"})),
            (
                "task_block",
                json!({"type":"missing_information","reason":"Input required"}),
            ),
            ("task_release", json!({})),
            ("task_request_review", json!({})),
            (
                "task_complete",
                json!({"summary":"Done","contractChanges":[],"outOfScopeEdits":[]}),
            ),
            (
                "loop_set",
                json!({"prompt":"Continue","interval":5,"maxRounds":10,"stopWhen":"round_limit","enabled":true}),
            ),
            (
                "writing_structure",
                json!({"fileId":"file-1","revision":"1"}),
            ),
            (
                "writing_variant_add",
                json!({"fileId":"file-1","lineId":"element-3","baseRevision":"1","text":"Another take."}),
            ),
        ] {
            assert!(
                service_call_succeeded(&mut agent, name, arguments),
                "{name} did not produce protocol-valid parameters"
            );
        }
        assert_web_tools(&mut agent);
        let web_open = agent
            .engine
            .calls
            .iter()
            .find(|(method, _)| method == "web.open")
            .map(|(_, params)| params)
            .unwrap();
        assert_eq!(web_open["workspaceId"], "workspace");
        assert!(web_open["idempotencyKey"].as_str().is_some());
        let snapshot = agent
            .engine
            .calls
            .iter()
            .find(|(method, _)| method == "web.snapshot")
            .map(|(_, params)| params)
            .unwrap();
        assert!(snapshot.get("idempotencyKey").is_none());

        let mut chief = Service::new(
            FakeEngine {
                role: Role::Chief,
                calls: VecDeque::new(),
            },
            context(false),
        );
        for (name, arguments) in [
            (
                "task_create",
                json!({"title":"Build","mustNotChange":"Existing behavior","executor":{"workspaceDefault":true}}),
            ),
            ("task_show", json!({"taskId":"WS-1"})),
            ("task_list", json!({"limit":20})),
            (
                "task_update",
                json!({"taskId":"WS-1","version":1,"priority":"high"}),
            ),
            (
                "task_comment",
                json!({"taskId":"WS-1","text":"Review this"}),
            ),
            (
                "task_dispatch",
                json!({"taskId":"WS-1","preset":{"workspaceDefault":true}}),
            ),
            ("task_unblock", json!({"taskId":"WS-1"})),
            (
                "loop_set",
                json!({"prompt":"Coordinate","interval":5,"maxRounds":10,"stopWhen":"three_empty","enabled":true}),
            ),
        ] {
            assert!(
                service_call_succeeded(&mut chief, name, arguments),
                "{name} did not produce protocol-valid parameters"
            );
        }
    }

    fn assert_web_tools(service: &mut Service<FakeEngine>) {
        for (name, arguments) in [
            (
                "web_open",
                json!({"url":"localhost:3000","workspaceId":"spoofed"}),
            ),
            ("web_navigate", json!({"paneId":"web","action":"reload"})),
            ("web_snapshot", json!({"paneId":"web","screenshot":true})),
            ("web_click", json!({"paneId":"web","ref":"e1"})),
            (
                "web_type",
                json!({"paneId":"web","ref":"e1","text":"Hello","submit":true}),
            ),
            ("web_press", json!({"paneId":"web","key":"Enter"})),
            ("web_hover", json!({"paneId":"web","ref":"e2"})),
            (
                "web_select",
                json!({"paneId":"web","ref":"e3","value":"one"}),
            ),
            (
                "web_scroll",
                json!({"paneId":"web","ref":"e4","dx":0,"dy":600}),
            ),
            (
                "web_wait",
                json!({"paneId":"web","text":"Ready","timeoutMs":5000}),
            ),
            (
                "web_evaluate",
                json!({"paneId":"web","js":"document.title"}),
            ),
            ("web_screenshot", json!({"paneId":"web","fullPage":false})),
            ("web_console", json!({"paneId":"web","since":"0"})),
            ("web_network", json!({"paneId":"web","since":"0"})),
            ("web_resize", json!({"paneId":"web","preset":"responsive"})),
            ("web_close", json!({"paneId":"web"})),
        ] {
            assert!(
                service_call_succeeded(service, name, arguments),
                "{name} did not produce protocol-valid parameters"
            );
        }
    }

    fn service_call_succeeded(
        service: &mut Service<FakeEngine>,
        name: &str,
        arguments: Value,
    ) -> bool {
        service.call(name, arguments).is_ok()
    }

    struct ImageEngine;

    impl EnginePort for ImageEngine {
        fn role(&self) -> &Role {
            &Role::Agent
        }

        fn request(&mut self, _method: &str, _params: Value) -> Result<Value> {
            Err(Error::UnexpectedFrame)
        }

        fn request_with_attachment(
            &mut self,
            method: &str,
            _params: Value,
        ) -> Result<EngineResponse> {
            assert_eq!(method, "web.screenshot");
            let reference = AttachmentRef {
                attachment_id: "00112233-4455-4677-8899-aabbccddeeff".into(),
                mime_type: "image/png".into(),
                name: "screenshot.png".into(),
                bytes: "3".into(),
            };
            Ok(EngineResponse {
                value: json!({"paneId":"web","image":reference}),
                attachment: Some(EngineAttachment {
                    reference,
                    bytes: b"png".to_vec(),
                }),
            })
        }
    }

    #[test]
    fn screenshot_tool_returns_mcp_image_content_and_structured_reference() {
        let mut service = Service::new(ImageEngine, context(false));
        let result = service
            .call("web_screenshot", json!({"paneId":"web"}))
            .unwrap();
        assert_eq!(result["content"][1]["type"], "image");
        assert_eq!(result["content"][1]["mimeType"], "image/png");
        assert_eq!(result["content"][1]["data"], "cG5n");
        assert_eq!(result["structuredContent"]["image"]["bytes"], "3");
    }
}
