use crate::{Error, Result, engine::EnginePort};
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

const TOOLS: [Tool; 15] = [
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
        let result = self.engine.request(tool.method, params)?;
        let text = serde_json::to_string(&result)?;
        Ok(json!({
            "content":[{"type":"text","text":text}],
            "structuredContent":result,
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
    !matches!(method, "task.show" | "task.list")
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
            (
                "permission",
                json!({"type":"string","enum":["ask","accept_edits","allow_all"]}),
            ),
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

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(agent_names.contains(&"task_next".into()));
        assert!(agent_names.contains(&"task_complete".into()));
        assert!(!agent_names.contains(&"task_dispatch".into()));
        assert!(chief_names.contains(&"task_dispatch".into()));
        assert!(chief_names.contains(&"task_list".into()));
        assert!(!chief_names.contains(&"task_complete".into()));
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
        ] {
            assert!(service_call_succeeded(&mut agent, name, arguments));
        }

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
            assert!(service_call_succeeded(&mut chief, name, arguments));
        }
    }

    fn service_call_succeeded(
        service: &mut Service<FakeEngine>,
        name: &str,
        arguments: Value,
    ) -> bool {
        service.call(name, arguments).is_ok()
    }
}
