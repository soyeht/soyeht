use crate::{Error, Result, engine::EnginePort};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_protocol::{Role, limits, method};
use uuid::Uuid;

mod schemas;
use schemas::*;

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

const TOOLS: [Tool; 36] = [
    Tool {
        name: "list_agents",
        method: method::AGENT_LIST,
        audience: Audience::Both,
        description: "List other ACP agent panes in this workspace, including IDs for messaging.",
        schema: empty_schema,
    },
    Tool {
        name: "message_agent",
        method: method::AGENT_MESSAGE,
        audience: Audience::Both,
        description: "Queue a message for another ACP agent; delivery waits for its clean input.",
        schema: agent_message_schema,
    },
    Tool {
        name: "task_create",
        method: method::TASK_CREATE,
        audience: Audience::Chief,
        description: "Create a workspace task with explicit preservation constraints.",
        schema: create_schema,
    },
    Tool {
        name: "task_next",
        method: method::TASK_NEXT,
        audience: Audience::Agent,
        description: "Claim the next eligible task for this pane.",
        schema: empty_schema,
    },
    Tool {
        name: "task_show",
        method: method::TASK_SHOW,
        audience: Audience::Both,
        description: "Show the current task or a selected workspace task.",
        schema: task_selector_schema,
    },
    Tool {
        name: "task_list",
        method: method::TASK_LIST,
        audience: Audience::Chief,
        description: "List workspace tasks.",
        schema: list_schema,
    },
    Tool {
        name: "task_update",
        method: method::TASK_UPDATE,
        audience: Audience::Both,
        description: "Update task fields using its current version.",
        schema: update_schema,
    },
    Tool {
        name: "task_comment",
        method: method::TASK_COMMENT,
        audience: Audience::Both,
        description: "Add a durable task comment.",
        schema: comment_schema,
    },
    Tool {
        name: "task_subitem_add",
        method: method::TASK_SUBITEM_ADD,
        audience: Audience::Agent,
        description: "Add a task subitem before executing it.",
        schema: subitem_add_schema,
    },
    Tool {
        name: "task_subitem_done",
        method: method::TASK_SUBITEM_DONE,
        audience: Audience::Agent,
        description: "Mark one task subitem as done.",
        schema: subitem_done_schema,
    },
    Tool {
        name: "task_block",
        method: method::TASK_BLOCK,
        audience: Audience::Agent,
        description: "Block the task with a typed reason.",
        schema: block_schema,
    },
    Tool {
        name: "task_release",
        method: method::TASK_RELEASE,
        audience: Audience::Agent,
        description: "Return the current task to the workspace list.",
        schema: release_schema,
    },
    Tool {
        name: "task_request_review",
        method: method::TASK_REQUEST_REVIEW,
        audience: Audience::Agent,
        description: "Request review after all subitems are done.",
        schema: release_schema,
    },
    Tool {
        name: "task_complete",
        method: method::TASK_COMPLETE,
        audience: Audience::Agent,
        description: "Complete the task with explicit contract and scope declarations.",
        schema: complete_schema,
    },
    Tool {
        name: "task_dispatch",
        method: method::TASK_DISPATCH,
        audience: Audience::Chief,
        description: "Dispatch a task with an executor preset.",
        schema: dispatch_schema,
    },
    Tool {
        name: "task_unblock",
        method: method::TASK_UNBLOCK,
        audience: Audience::Chief,
        description: "Remove a task block with a recorded reason.",
        schema: unblock_schema,
    },
    Tool {
        name: "loop_set",
        method: method::LOOP_SET,
        audience: Audience::Both,
        description: "Configure the loop for this pane.",
        schema: loop_schema,
    },
    Tool {
        name: "writing_structure",
        method: method::WRITING_STRUCTURE,
        audience: Audience::Both,
        description: "Read the bounded structure of an open writing document.",
        schema: writing_structure_schema,
    },
    Tool {
        name: "writing_variant_add",
        method: method::WRITING_VARIANT_ADD,
        audience: Audience::Both,
        description: "Add one alternate take to a screenplay line for the writer to choose.",
        schema: writing_variant_add_schema,
    },
    Tool {
        name: "web_open",
        method: method::WEB_OPEN,
        audience: Audience::Both,
        description: "Open or focus a web pane in the bound workspace and return its paneId. An address that a web pane already shows returns that pane, which is how to get the paneId of a page the person has open; every other web tool needs a paneId.",
        schema: web_open_schema,
    },
    Tool {
        name: "web_navigate",
        method: method::WEB_NAVIGATE,
        audience: Audience::Both,
        description: "Navigate a web pane to a URL or through its local history.",
        schema: web_navigate_schema,
    },
    Tool {
        name: "web_snapshot",
        method: method::WEB_SNAPSHOT,
        audience: Audience::Both,
        description: "Read a bounded snapshot of a visible web pane: its controls, headings, text and link addresses. Needs the paneId from web_open.",
        schema: web_snapshot_schema,
    },
    Tool {
        name: "web_click",
        method: method::WEB_CLICK,
        audience: Audience::Both,
        description: "Click an element reference from the latest web snapshot.",
        schema: web_ref_schema,
    },
    Tool {
        name: "web_type",
        method: method::WEB_TYPE,
        audience: Audience::Both,
        description: "Type bounded text into an element reference.",
        schema: web_type_schema,
    },
    Tool {
        name: "web_press",
        method: method::WEB_PRESS,
        audience: Audience::Both,
        description: "Send one bounded key name to a web pane.",
        schema: web_press_schema,
    },
    Tool {
        name: "web_hover",
        method: method::WEB_HOVER,
        audience: Audience::Both,
        description: "Hover an element reference from the latest web snapshot.",
        schema: web_ref_schema,
    },
    Tool {
        name: "web_select",
        method: method::WEB_SELECT,
        audience: Audience::Both,
        description: "Select a bounded value in a referenced web control.",
        schema: web_select_schema,
    },
    Tool {
        name: "web_scroll",
        method: method::WEB_SCROLL,
        audience: Audience::Both,
        description: "Scroll a web pane or a referenced element by bounded deltas.",
        schema: web_scroll_schema,
    },
    Tool {
        name: "web_wait",
        method: method::WEB_WAIT,
        audience: Audience::Both,
        description: "Wait for bounded text, selector, or network-idle conditions.",
        schema: web_wait_schema,
    },
    Tool {
        name: "web_evaluate",
        method: method::WEB_EVALUATE,
        audience: Audience::Both,
        description: "Evaluate bounded page code after a separate human permission.",
        schema: web_evaluate_schema,
    },
    Tool {
        name: "web_screenshot",
        method: method::WEB_SCREENSHOT,
        audience: Audience::Both,
        description: "Capture a bounded screenshot from a visible web pane. With fullPage it covers at most the read limit of screens and endOfPage says whether the page ended.",
        schema: web_screenshot_schema,
    },
    Tool {
        name: "web_read",
        method: method::WEB_READ,
        audience: Audience::Both,
        description: "Read a web pane like a person: scroll down natively for a bounded number of screens, then return one stitched image plus the visible text and links. Needs the paneId from web_open.",
        schema: web_read_schema,
    },
    Tool {
        name: "web_console",
        method: method::WEB_CONSOLE,
        audience: Audience::Both,
        description: "Read bounded console metadata without page or transport bodies.",
        schema: web_since_schema,
    },
    Tool {
        name: "web_network",
        method: method::WEB_NETWORK,
        audience: Audience::Both,
        description: "Read bounded network metadata without headers or bodies.",
        schema: web_since_schema,
    },
    Tool {
        name: "web_resize",
        method: method::WEB_RESIZE,
        audience: Audience::Both,
        description: "Resize a visible web pane using a preset or explicit dimensions.",
        schema: web_resize_schema,
    },
    Tool {
        name: "web_close",
        method: method::WEB_CLOSE,
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
            if attachment.bytes.len() <= limits::WEB_AGENT_IMAGE_BYTES {
                content.push(json!({
                    "type":"image",
                    "data":STANDARD.encode(attachment.bytes),
                    "mimeType":attachment.reference.mime_type
                }));
            } else {
                // A harness drops its connection on a message longer than its protocol allows,
                // and carries the image in it as base64 and may carry it twice: a picture over
                // the limit (a host that does not keep to it) is left out, and the agent is told.
                content.push(json!({
                    "type":"text",
                    "text":"The page picture is larger than a tool result can carry and was left out; the text and links hold the page."
                }));
            }
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
        method::TASK_SHOW
            | method::AGENT_LIST
            | method::TASK_LIST
            | method::WRITING_STRUCTURE
            | method::WEB_SNAPSHOT
            | method::WEB_WAIT
            | method::WEB_SCREENSHOT
            | method::WEB_READ
            | method::WEB_CONSOLE
            | method::WEB_NETWORK
    )
}

#[cfg(test)]
mod tests;
