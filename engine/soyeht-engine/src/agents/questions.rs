use super::{Changes, forms, state::State};
use crate::{
    core::Core,
    errors::{self, Result},
    model::StoredPane,
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_acp::client;
use soyeht_protocol::{
    PaneState, PermissionFileChange, PermissionOption, PermissionOptionKind,
    PermissionPresentation, PermissionToolArgument, Role, limits,
};
use soyeht_store::agents::NativeRequest;
use std::collections::BTreeMap;
use uuid::Uuid;

/// Display names of the panes in the asking pane's workspace, by pane id.
pub type PaneNames<'a> = BTreeMap<&'a str, &'a str>;

pub const QUESTION_TIMEOUT_MS: i64 = limits::PERMISSION_TIMEOUT_SEC as i64 * 1_000;

pub struct Incoming<'a> {
    pub source_seq: u64,
    pub native_id: Value,
    pub method: &'a str,
    pub params: Value,
    pub now_ms: i64,
    pub timestamp: &'a str,
    pub pane_names: &'a PaneNames<'a>,
    /// The latest state of the requested tool call as reported earlier in
    /// this prompt by session/update, when the agent reported it.
    pub tracked_tool: Option<Value>,
}

pub struct Decision<'a> {
    pub option: Option<&'a str>,
    pub fields: Option<Value>,
    pub by: &'a str,
    pub expired: bool,
    pub timestamp: &'a str,
}

fn invalid(message: &str) -> crate::errors::Error {
    errors::invalid(message)
}

fn bounded(text: &str, maximum: usize) -> Result<String> {
    if text.chars().count() > maximum {
        return Err(invalid("Native question exceeds public protocol bounds"));
    }
    Ok(text.into())
}

/// An MCP tool call awaiting approval. Harnesses describe it differently:
/// codex-acp sends kind "execute" with rawInput {server, tool, arguments} and
/// no command; claude-acp names the tool `mcp__<server>__<tool>` and sends the
/// arguments as rawInput. Both are presented the same way.
struct McpToolCall<'a> {
    server: &'a str,
    tool: &'a str,
    arguments: Option<&'a serde_json::Map<String, Value>>,
}

impl<'a> McpToolCall<'a> {
    fn recognise(tool: &'a Value) -> Option<Self> {
        let input = &tool["rawInput"];
        if let (Some(server), Some(name), None) = (
            input["server"].as_str(),
            input["tool"].as_str(),
            input.get("command"),
        ) {
            let arguments = match input.get("arguments") {
                None => None,
                Some(arguments) => Some(arguments.as_object()?),
            };
            return Some(Self {
                server,
                tool: name,
                arguments,
            });
        }
        let (server, name) = tool["name"]
            .as_str()?
            .strip_prefix("mcp__")?
            .split_once("__")?;
        Some(Self {
            server,
            tool: name,
            arguments: Some(input.as_object()?),
        })
    }

    fn title(&self) -> String {
        format!("Use {} · {}", self.server, self.tool)
    }

    /// Every argument is shown in full, or the request is not presented.
    fn arguments(&self, pane_names: &PaneNames<'_>) -> Result<Vec<PermissionToolArgument>> {
        let mut bytes = 0;
        let mut arguments = Vec::new();
        for (name, value) in self.arguments.into_iter().flatten() {
            let value = match value {
                Value::String(text) => pane_names
                    .get(text.as_str())
                    .map_or_else(|| text.clone(), |pane| (*pane).to_owned()),
                Value::Array(_) | Value::Object(_) => serde_json::to_string_pretty(value)
                    .map_err(|_| errors::internal("Tool argument serialization failed"))?,
                scalar => scalar.to_string(),
            };
            bytes += name.len() + value.len();
            if bytes > limits::PERMISSION_COMMAND_BYTES {
                return Err(invalid("Tool request arguments exceed display limit"));
            }
            arguments.push(PermissionToolArgument {
                name: name.clone(),
                value,
            });
        }
        Ok(arguments)
    }
}

/// codex-acp asks to approve an MCP tool call with only its id, kind and
/// status; the server, tool and arguments came in the earlier tool_call
/// update. The request is judged on the tracked state overlaid with every
/// field the request itself carries, so the request always has the last word.
fn requested_tool_call(tracked: Option<&Value>, requested: &Value) -> Value {
    let (Some(Value::Object(tracked)), Value::Object(fields)) = (tracked, requested) else {
        return requested.clone();
    };
    let mut merged = tracked.clone();
    merged.extend(fields.clone());
    Value::Object(merged)
}

fn permission_presentation(
    tool: &Value,
    mcp: Option<&McpToolCall<'_>>,
    pane_names: &PaneNames<'_>,
) -> Result<PermissionPresentation> {
    let mut execution_details = Vec::new();
    if tool["kind"] == "execute" {
        let mut content_bytes = 0;
        for item in tool["content"].as_array().into_iter().flatten() {
            if item["type"] == "content" && item["content"]["type"] == "text" {
                let Some(text) = item["content"]["text"].as_str() else {
                    continue;
                };
                content_bytes += text.len();
                if content_bytes > limits::PERMISSION_COMMAND_BYTES {
                    return Err(invalid(
                        "Executable permission content exceeds display limit",
                    ));
                }
                execution_details.push(text.to_owned());
            }
        }
    }
    let command = if tool["kind"] == "execute" && mcp.is_none() {
        // The claude-acp adapter puts a human summary in title/content; only
        // rawInput.command is the shell command that would actually run.
        let value = tool["rawInput"]["command"]
            .as_str()
            .filter(|command| !command.trim().is_empty())
            .ok_or_else(|| invalid("Executable permission has no command to display"))?;
        if value.len() > limits::PERMISSION_COMMAND_BYTES {
            return Err(invalid(
                "Executable permission command exceeds display limit",
            ));
        }
        let command = value.to_owned();
        execution_details
            .retain(|detail| detail != &command && tool["title"].as_str() != Some(detail.as_str()));
        Some(command)
    } else {
        None
    };
    let target_path = tool["locations"]
        .as_array()
        .and_then(|locations| locations.iter().find_map(|item| item["path"].as_str()))
        .filter(|path| path.chars().count() <= limits::PERMISSION_PREVIEW_PATH_CHARS)
        .map(str::to_owned);
    let mut file_changes = Vec::new();
    let mut preview_omitted = false;
    let mut preview_bytes = 0;
    for item in tool["content"].as_array().into_iter().flatten() {
        if item["type"] != "diff" {
            continue;
        }
        let Some(path) = item["path"].as_str() else {
            preview_omitted = true;
            continue;
        };
        let Some(new_text) = item["newText"].as_str() else {
            preview_omitted = true;
            continue;
        };
        let old_text = item["oldText"].as_str();
        preview_bytes += path.len() + new_text.len() + old_text.map_or(0, str::len);
        if file_changes.len() >= limits::PERMISSION_PREVIEW_CHANGES
            || path.chars().count() > limits::PERMISSION_PREVIEW_PATH_CHARS
            || preview_bytes > limits::PERMISSION_PREVIEW_BYTES
        {
            // An incomplete diff must never look like a complete approval preview.
            file_changes.clear();
            preview_omitted = true;
            break;
        }
        file_changes.push(PermissionFileChange {
            path: path.into(),
            old_text: old_text.map(str::to_owned),
            new_text: new_text.into(),
        });
    }
    Ok(PermissionPresentation {
        target_path: target_path.or_else(|| file_changes.first().map(|change| change.path.clone())),
        command,
        execution_details: (!execution_details.is_empty()).then_some(execution_details),
        tool_arguments: mcp.map(|call| call.arguments(pane_names)).transpose()?,
        file_changes,
        preview_omitted,
    })
}

fn public_question(
    method: &str,
    params: &Value,
    tracked_tool: Option<&Value>,
    mut public: Value,
    pane_names: &PaneNames<'_>,
) -> Result<(Value, &'static str)> {
    let event = if method == "session/request_permission" {
        let options: Vec<PermissionOption> = serde_json::from_value(params["options"].clone())
            .map_err(|_| invalid("Invalid native permission options"))?;
        if options.is_empty()
            || options.len() > 32
            || options.iter().any(|option| {
                option.option_id.is_empty()
                    || option.option_id.chars().count() > 128
                    || option.name.chars().count() > 256
            })
            || options.iter().enumerate().any(|(index, item)| {
                options[..index]
                    .iter()
                    .any(|other| other.option_id == item.option_id)
            })
        {
            return Err(invalid(
                "Invalid native permission option identity or bounds",
            ));
        }
        // Persistent native grants may extend beyond this pane. Only expose
        // them when an equivalent one-time choice can enforce pane ownership.
        let public_options: Vec<_> = options
            .iter()
            .filter(|option| match option.kind {
                PermissionOptionKind::AllowAlways => options
                    .iter()
                    .any(|other| other.kind == PermissionOptionKind::AllowOnce),
                PermissionOptionKind::RejectAlways => options
                    .iter()
                    .any(|other| other.kind == PermissionOptionKind::RejectOnce),
                _ => true,
            })
            .collect();
        let metadata = &params["_meta"]["permission"];
        let tool = requested_tool_call(tracked_tool, &params["toolCall"]);
        let mcp = McpToolCall::recognise(&tool);
        let title = match &mcp {
            Some(call) => call.title(),
            None => if metadata["version"] == 1 {
                metadata["title"].as_str()
            } else {
                None
            }
            .or_else(|| tool["title"].as_str())
            .unwrap_or("Permission required")
            .to_owned(),
        };
        let presentation = permission_presentation(&tool, mcp.as_ref(), pane_names)?;
        public["title"] = json!(if tool["kind"] == "execute"
            && mcp.is_none()
            && title.chars().count() > 512
        {
            "Run command".to_owned()
        } else {
            bounded(&title, 512)?
        });
        public["detail"] = json!(if metadata["version"] == 1 {
            metadata["description"]
                .as_str()
                .filter(|text| text.chars().count() <= 512)
                .unwrap_or("")
        } else {
            ""
        });
        public["presentation"] = serde_json::to_value(presentation)
            .map_err(|_| errors::internal("Permission presentation serialization failed"))?;
        public["options"] = json!(public_options);
        "pane.permission_requested"
    } else {
        if method != "elicitation/create"
            || params["mode"] != "form"
            || !forms::supported(&params["requestedSchema"], 0)
        {
            return Err(invalid("Unsupported elicitation form"));
        }
        public["title"] = json!(bounded(
            params["message"]
                .as_str()
                .ok_or_else(|| invalid("Form message missing"))?,
            512
        )?);
        public["schema"] = params["requestedSchema"].clone();
        "pane.elicitation_requested"
    };
    if errors::json_bytes(&public)?.len() > 512 * 1024 {
        return Err(invalid("Question exceeds snapshot item capacity"));
    }
    Ok((public, event))
}

pub fn receive(
    state: &mut State,
    saved: &mut StoredPane,
    incoming: Incoming<'_>,
    changes: &mut Changes,
) -> Result<()> {
    let Incoming {
        source_seq,
        native_id,
        method,
        params,
        now_ms,
        timestamp,
        pane_names,
        tracked_tool,
    } = incoming;
    if !state.owns_session(params["sessionId"].as_str()) {
        return Err(invalid("Question belongs to another session"));
    }
    let prompt = state
        .active
        .as_ref()
        .ok_or_else(|| invalid("Question has no active prompt"))?;
    let expires_ms = now_ms + QUESTION_TIMEOUT_MS;
    let request_id = Uuid::new_v4().to_string();
    let expires_at = chrono::DateTime::from_timestamp_millis(expires_ms)
        .ok_or_else(|| errors::internal("Question deadline is outside the timestamp range"))?
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let public = json!({"paneId":saved.pane.pane_id,"requestId":request_id,
        "generation":saved.pane.generation,"promptId":prompt.prompt_id,"expiresAt":expires_at});
    let (public, event) =
        public_question(method, &params, tracked_tool.as_ref(), public, pane_names)?;
    changes.received.push(NativeRequest {
        request_id,
        pane_id: saved.pane.pane_id.clone(),
        generation: saved.pane.generation,
        source_seq,
        native_id,
        method: method.into(),
        params,
        public: public.clone(),
        expires_ms,
        result: None,
    });
    let request =
        errors::invariant(changes.received.last(), "Received question was not staged")?.clone();
    if let Some(option) = automatic(state, &request, &saved.pane.cwd)? {
        decision(
            state,
            &request,
            Decision {
                option: Some(&option),
                fields: None,
                by: "rule",
                expired: false,
                timestamp,
            },
            changes,
        )?;
        if saved.pane.state != PaneState::WaitingForYou {
            saved.pane.state = PaneState::Working;
            saved.pane.phrase = "Agent is working".into();
        }
    } else {
        changes.events.push((event.into(), public));
        saved.pane.state = PaneState::WaitingForYou;
        saved.pane.phrase = "Waiting for your answer".into();
    }
    Ok(())
}

/// A persistent grant covers the request as the agent sent it and exactly
/// what the person was shown. A request that carries only a tool call id
/// (codex-acp MCP approvals) is told apart by its presented server, tool and
/// arguments, so one grant never extends to another tool. A subagent's grant
/// is its own: it never covers the parent's requests, nor the parent's its.
fn rule_key(state: &State, request: &NativeRequest) -> Result<String> {
    let mut tool = request.params["toolCall"].clone();
    if let Some(object) = tool.as_object_mut() {
        object.remove("toolCallId");
        object.remove("status");
    }
    let mut granted = json!([
        state.launch.id,
        tool,
        request.public["title"],
        request.public["presentation"],
    ]);
    let session = request.params["sessionId"].as_str();
    if session != state.session_id.as_deref()
        && let Some(parts) = granted.as_array_mut()
    {
        parts.push(json!({"subagentSession": session}));
    }
    use sha2::{Digest, Sha256};
    Ok(format!(
        "{:x}",
        Sha256::digest(errors::json_bytes(&granted)?)
    ))
}

/// Permission policy never handles elicitation. Native persistent grants are
/// translated to one-time answers; their rule is owned by this pane alone.
/// Beyond those grants the engine approves only under its own modes, which
/// exist just for agents that announce no native permission modes.
pub fn automatic(state: &State, request: &NativeRequest, cwd: &str) -> Result<Option<String>> {
    if request.method != "session/request_permission" {
        return Ok(None);
    }
    let rule = rule_key(state, request)?;
    let policy = super::configuration::engine_policy(state);
    let allowed = state.permission_rules.get(&rule).copied().or_else(|| {
        let edit = policy == Some(super::configuration::ACCEPT_EDITS)
            && request.params["toolCall"]["kind"] == "edit"
            && request.params["toolCall"]["locations"]
                .as_array()
                .is_some_and(|paths| {
                    !paths.is_empty()
                        && paths.iter().all(|location| {
                            location["path"].as_str().is_some_and(|path| {
                                super::filesystem::permits_edit(
                                    std::path::Path::new(cwd),
                                    std::path::Path::new(path),
                                )
                            })
                        })
                });
        if policy == Some(super::configuration::ALLOW_ALL) || edit {
            Some(true)
        } else {
            None
        }
    });
    let Some(allowed) = allowed else {
        return Ok(None);
    };
    let kind = if allowed { "allow_once" } else { "reject_once" };
    Ok(request.public["options"].as_array().and_then(|options| {
        options
            .iter()
            .find(|option| option["kind"] == kind)
            .and_then(|option| option.get("optionId"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    }))
}

pub fn decision(
    state: &mut State,
    request: &NativeRequest,
    decision: Decision<'_>,
    changes: &mut Changes,
) -> Result<Value> {
    let Decision {
        option,
        fields,
        by,
        expired,
        timestamp,
    } = decision;
    if !matches!(
        request.method.as_str(),
        "session/request_permission" | "elicitation/create"
    ) {
        super::effects::finish(
            request,
            Err(super::effects::failure(if expired {
                "Native request expired"
            } else {
                "Native request cancelled"
            })),
            changes,
        );
        return Ok(json!({}));
    }
    let permission = request.method == "session/request_permission";
    if expired {
        changes.attention.push(soyeht_protocol::AttentionItem {
            attention_id: request.request_id.clone(),
            pane_id: request.pane_id.clone(),
            request_id: None,
            message: if permission {
                "Permission expired. Send a new prompt to resume."
            } else {
                "Question expired. Send a new prompt to resume."
            }
            .into(),
            created_at: timestamp.into(),
        });
    }
    let result_name = if expired {
        "expired"
    } else if permission && option.is_some() {
        "selected"
    } else if !permission && fields.is_some() {
        "submitted"
    } else {
        "cancelled"
    };
    let mut receipt = json!({"requestId":request.request_id,"result":result_name,"by":by});
    let native = if permission {
        let mut selected = option.map(str::to_owned);
        if let Some(option) = option {
            let options = request.public["options"]
                .as_array()
                .ok_or_else(|| invalid("Stored permission options missing"))?;
            let item = options
                .iter()
                .find(|o| o["optionId"] == option)
                .ok_or_else(|| invalid("Unknown permission option"))?;
            receipt["optionId"] = json!(option);
            if matches!(
                item["kind"].as_str(),
                Some("allow_always" | "reject_always")
            ) {
                let allow = item["kind"] == "allow_always";
                let kind = if allow { "allow_once" } else { "reject_once" };
                selected = Some(
                    options
                        .iter()
                        .find(|o| o["kind"] == kind)
                        .and_then(|o| o["optionId"].as_str())
                        .ok_or_else(|| invalid("Persistent choice has no pane-scoped equivalent"))?
                        .into(),
                );
                let rule = rule_key(state, request)?;
                if state.permission_rules.len() >= 128
                    && !state.permission_rules.contains_key(&rule)
                {
                    return Err(invalid("Pane permission rule limit reached"));
                }
                state.permission_rules.insert(rule, allow);
            }
        }
        client::permission_answer(request.native_id.clone(), selected.as_deref())
    } else {
        if fields
            .as_ref()
            .is_some_and(|fields| !forms::valid(&request.public["schema"], fields))
        {
            return Err(invalid("Answer does not satisfy the requested form"));
        }
        client::elicitation_answer(request.native_id.clone(), fields)
    };
    changes.answers.push((
        request.request_id.clone(),
        json!({"receipt":receipt,"native":native.value()}),
    ));
    changes.outgoing.push(native);
    let mut event = receipt.clone();
    event["paneId"] = json!(request.pane_id);
    changes.events.push((
        if permission {
            "pane.permission_answered"
        } else {
            "pane.elicitation_answered"
        }
        .into(),
        event,
    ));
    Ok(receipt)
}

impl Core {
    pub fn agent_answer(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        let pane = errors::param_str(params, "paneId")?;
        let mut state = self.agent_state(pane)?;
        let mut saved = self.panes[pane].stored.clone();
        let request = self
            .store
            .agent_request(errors::param_str(params, "requestId")?)
            .map_err(errors::storage)?
            .filter(|r| r.pane_id == pane)
            .ok_or_else(|| errors::not_found("Question not found"))?;
        let expected = if method == "pane.permission.answer" {
            "session/request_permission"
        } else {
            "elicitation/create"
        };
        if request.method != expected {
            return Err(invalid("Wrong answer method for this question"));
        }
        if let Some(result) = request.result {
            return Ok(self
                .commit_agent(
                    saved,
                    state,
                    Changes::default(),
                    Some((connection, method, params)),
                    result["receipt"].clone(),
                )?
                .result);
        }
        if request.generation != saved.pane.generation {
            return Err(invalid("Question belongs to an earlier pane generation"));
        }
        let expired = request.expires_ms <= self.now_ms();
        let by = if expired {
            "system"
        } else if self.clients[connection]
            .actor
            .as_ref()
            .is_some_and(|a| a.role == Role::HumanDevice)
        {
            "iphone"
        } else {
            "mac"
        };
        let option = if expired {
            None
        } else {
            params["optionId"].as_str()
        };
        let fields = if expired || params["cancel"] == true {
            None
        } else {
            params.get("fields").cloned()
        };
        if !expired
            && expected == "elicitation/create"
            && params["cancel"] != true
            && fields.is_none()
        {
            return Err(invalid("Provide form fields or cancel the question"));
        }
        let mut changes = Changes::default();
        let result = decision(
            &mut state,
            &request,
            Decision {
                option,
                fields,
                by,
                expired,
                timestamp: &self.timestamp(),
            },
            &mut changes,
        )?;
        let other_open = self
            .store
            .agent_open_requests(pane)
            .map_err(errors::storage)?
            .iter()
            .any(|r| r.request_id != request.request_id);
        if !other_open && state.active.is_some() {
            saved.pane.state = PaneState::Working;
            saved.pane.phrase = "Agent is working".into();
        }
        Ok(self
            .commit_agent(
                saved,
                state,
                changes,
                Some((connection, method, params)),
                result,
            )?
            .result)
    }
}

#[cfg(test)]
mod presentation_tests {
    use super::*;

    #[test]
    fn permission_exposes_typed_diff_without_raw_tool_input() {
        let params = json!({
            "toolCall": {
                "toolCallId": "private-id", "title": "Write note.txt", "kind": "edit",
                "rawInput": {"token": "private-token"},
                "locations": [{"path": "/project/note.txt"}],
                "content": [{"type": "diff", "path": "/project/note.txt",
                    "oldText": "before\n", "newText": "after\n"}]
            },
            "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}],
            "_meta": {"permission": {"version": 1, "title": "Write note.txt",
                "description": "Review this edit"}}
        });
        let (public, event) = public_question(
            "session/request_permission",
            &params,
            None,
            json!({}),
            &PaneNames::new(),
        )
        .expect("valid permission");
        assert_eq!(event, "pane.permission_requested");
        assert_eq!(public["title"], "Write note.txt");
        assert_eq!(public["detail"], "Review this edit");
        assert_eq!(public["presentation"]["targetPath"], "/project/note.txt");
        assert_eq!(
            public["presentation"]["fileChanges"][0]["oldText"],
            "before\n"
        );
        assert_eq!(
            public["presentation"]["fileChanges"][0]["newText"],
            "after\n"
        );
        let encoded = public.to_string();
        assert!(!encoded.contains("private-token"));
        assert!(!encoded.contains("private-id"));
        assert!(!encoded.contains("rawInput"));
    }

    #[test]
    fn large_diff_is_not_presented_as_a_complete_preview() {
        let tool = json!({"content": [{"type": "diff", "path": "/project/large.txt",
            "newText": "x".repeat(limits::PERMISSION_PREVIEW_BYTES + 1)}]});
        let presentation =
            permission_presentation(&tool, None, &PaneNames::new()).expect("bounded presentation");
        assert!(presentation.preview_omitted);
        assert!(presentation.file_changes.is_empty());
    }

    #[test]
    fn claude_execute_permission_shows_raw_command_not_summary() {
        // Sanitized from the real claude-acp permission request observed on VM1.
        let command = "touch made-by-shell.txt && ls -la";
        let params = json!({
            "toolCall": {"kind": "execute", "title": "Create file and list directory contents",
                "rawInput": {"command": command, "description": "Create file and list directory contents",
                    "secret": "never-public"},
                "content": [{"type": "content", "content": {"type": "text",
                    "text": "Create file and list directory contents"}}]},
            "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}],
            "_meta": {"permission": {"version": 1,
                "title": "Create file and list directory contents",
                "description": "Review shell command"}}
        });
        let (public, _) = public_question(
            "session/request_permission",
            &params,
            None,
            json!({}),
            &PaneNames::new(),
        )
        .expect("complete command must be displayable");
        assert_eq!(public["title"], "Create file and list directory contents");
        assert_eq!(public["presentation"]["command"], command);
        assert_eq!(public["presentation"]["executionDetails"], Value::Null);
        assert!(!public.to_string().contains("never-public"));
        assert!(!public.to_string().contains("rawInput"));
    }

    #[test]
    fn execute_permission_preserves_long_multiline_command_and_extra_content() {
        let command = format!("ls -la\nprintf '%s' {}", "x".repeat(600));
        let params = json!({
            "toolCall": {"kind": "execute", "title": "List and print",
                "rawInput": {"command": command},
                "content": [{"type": "content", "content": {"type": "text",
                    "text": "Working directory: /project"}}]},
            "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]
        });
        let (public, _) = public_question(
            "session/request_permission",
            &params,
            None,
            json!({}),
            &PaneNames::new(),
        )
        .expect("complete command must be displayable");
        assert_eq!(public["presentation"]["command"], command);
        assert_eq!(
            public["presentation"]["executionDetails"][0],
            "Working directory: /project"
        );
    }

    #[test]
    fn codex_execute_permission_uses_adapter_command_not_generic_title() {
        // codex-acp 1.11.0 constructs command approvals with a string
        // rawInput.command, even when its internal launcher uses argv.
        let params = json!({
            "toolCall": {"kind": "execute", "title": "List files",
                "rawInput": {"command": "ls -la", "cwd": "/project"}},
            "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}],
            "_meta": {"permission": {"version": 1, "title": "Run command?"}}
        });
        let (public, _) = public_question(
            "session/request_permission",
            &params,
            None,
            json!({}),
            &PaneNames::new(),
        )
        .expect("Codex command must be displayable");
        assert_eq!(public["title"], "Run command?");
        assert_eq!(public["presentation"]["command"], "ls -la");
        assert_eq!(public["presentation"]["toolArguments"], Value::Null);
        assert!(!public.to_string().contains("/project"));
    }

    fn workspace_panes() -> PaneNames<'static> {
        PaneNames::from([("pane-claude", "Claude"), ("pane-codex", "Codex")])
    }

    #[test]
    fn codex_mcp_tool_approval_shows_server_tool_and_arguments() {
        // Shape of the codex-acp request recorded in a real Claude and Codex
        // run: an MCP call arrives as kind "execute" with no command.
        let params = json!({
            "toolCall": {"toolCallId": "exec-d712", "title": "mcp.soyeht-workspace.message_agent",
                "kind": "execute", "status": "pending",
                "rawInput": {"arguments": {"targetPaneId": "pane-claude", "text": "HTTP/2 200 \r\n"},
                    "server": "soyeht-workspace", "tool": "message_agent"}},
            "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]
        });
        let (public, _) = public_question(
            "session/request_permission",
            &params,
            None,
            json!({}),
            &workspace_panes(),
        )
        .expect("MCP tool approval must be displayable");
        assert_eq!(public["title"], "Use soyeht-workspace · message_agent");
        assert_eq!(public["presentation"]["command"], Value::Null);
        assert_eq!(
            public["presentation"]["toolArguments"],
            json!([{"name": "targetPaneId", "value": "Claude"},
                {"name": "text", "value": "HTTP/2 200 \r\n"}])
        );
        assert!(!public.to_string().contains("pane-claude"));
        assert!(!public.to_string().contains("rawInput"));
    }

    #[test]
    fn claude_and_codex_mcp_approvals_render_identically() {
        let arguments = json!({"targetPaneId": "pane-codex", "text": "Please rerun it.",
            "attempt": 2, "urgent": false, "tags": ["proof"]});
        let codex = json!({
            "toolCall": {"toolCallId": "exec-1", "title": "mcp.soyeht-workspace.message_agent",
                "kind": "execute", "status": "pending",
                "rawInput": {"arguments": arguments, "server": "soyeht-workspace",
                    "tool": "message_agent"}},
            "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]
        });
        // claude-acp names MCP tools and repeats the name as the title.
        let claude = json!({
            "toolCall": {"toolCallId": "toolu_1", "name": "mcp__soyeht-workspace__message_agent",
                "title": "mcp__soyeht-workspace__message_agent", "kind": "other",
                "status": "pending", "rawInput": arguments, "content": []},
            "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}],
            "_meta": {"permission": {"version": 1,
                "title": "mcp__soyeht-workspace__message_agent"}}
        });
        let present = |params: &Value| {
            public_question(
                "session/request_permission",
                params,
                None,
                json!({}),
                &workspace_panes(),
            )
            .expect("MCP tool approval must be displayable")
            .0
        };
        let (codex, claude) = (present(&codex), present(&claude));
        assert_eq!(claude["title"], "Use soyeht-workspace · message_agent");
        assert_eq!(codex["title"], claude["title"]);
        assert_eq!(codex["presentation"], claude["presentation"]);
        assert_eq!(
            claude["presentation"]["toolArguments"],
            json!([{"name": "attempt", "value": "2"},
                {"name": "tags", "value": "[\n  \"proof\"\n]"},
                {"name": "targetPaneId", "value": "Codex"},
                {"name": "text", "value": "Please rerun it."},
                {"name": "urgent", "value": "false"}])
        );
    }

    /// The codex-acp 1.11.0 approval for an MCP call carries only the id,
    /// kind and status; the call itself arrived in the earlier tool_call.
    fn bare_codex_mcp_request(id: &str) -> Value {
        json!({
            "toolCall": {"toolCallId": id, "kind": "execute", "status": "pending"},
            "_meta": {"is_mcp_tool_approval": true},
            "options": [{"optionId": "allow_once", "kind": "allow_once", "name": "Allow"}]
        })
    }

    #[test]
    fn codex_mcp_approval_is_presented_from_the_tracked_tool_call() {
        let tracked = json!({"toolCallId": "exec-a8b5", "title": "mcp.soyeht-workspace.message_agent",
            "kind": "execute", "status": "in_progress", "locations": [],
            "rawInput": {"arguments": {"targetPaneId": "pane-claude", "text": "  indented\n  reply"},
                "server": "soyeht-workspace", "tool": "message_agent"}});
        let (public, _) = public_question(
            "session/request_permission",
            &bare_codex_mcp_request("exec-a8b5"),
            Some(&tracked),
            json!({}),
            &workspace_panes(),
        )
        .expect("tracked MCP tool approval must be displayable");
        assert_eq!(public["title"], "Use soyeht-workspace · message_agent");
        assert_eq!(public["presentation"]["command"], Value::Null);
        assert_eq!(
            public["presentation"]["toolArguments"],
            json!([{"name": "targetPaneId", "value": "Claude"},
                {"name": "text", "value": "  indented\n  reply"}])
        );
    }

    #[test]
    fn request_fields_override_the_tracked_tool_call() {
        // The tracked call was reported as MCP, but the request itself names
        // a shell command: the request has the last word.
        let tracked = json!({"toolCallId": "exec-1", "title": "Tool", "kind": "execute",
            "status": "in_progress", "locations": [],
            "rawInput": {"arguments": {}, "server": "soyeht-workspace", "tool": "list_agents"}});
        let params = json!({
            "toolCall": {"toolCallId": "exec-1", "kind": "execute", "status": "pending",
                "title": "Run command", "rawInput": {"command": "ls", "cwd": "/project"}},
            "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]
        });
        let (public, _) = public_question(
            "session/request_permission",
            &params,
            Some(&tracked),
            json!({}),
            &workspace_panes(),
        )
        .expect("command must be displayable");
        assert_eq!(public["title"], "Run command");
        assert_eq!(public["presentation"]["command"], "ls");
        assert_eq!(public["presentation"]["toolArguments"], Value::Null);
    }

    #[test]
    fn untracked_bare_codex_approval_fails_closed() {
        let error = public_question(
            "session/request_permission",
            &bare_codex_mcp_request("exec-unknown"),
            None,
            json!({}),
            &workspace_panes(),
        )
        .expect_err("an approval with nothing to show must not be presented");
        assert!(error.to_string().contains("no command to display"));
        // A tracked call that is neither a command nor an MCP call stays closed too.
        let tracked = json!({"toolCallId": "exec-unknown", "title": "Tool", "kind": "execute",
            "status": "in_progress", "locations": [], "rawInput": {"server": "soyeht-workspace"}});
        assert!(
            public_question(
                "session/request_permission",
                &bare_codex_mcp_request("exec-unknown"),
                Some(&tracked),
                json!({}),
                &workspace_panes(),
            )
            .is_err()
        );
    }

    #[test]
    fn mcp_arguments_over_display_limit_fail_closed() {
        let params = json!({"toolCall": {"kind": "execute", "title": "mcp.server.tool",
            "rawInput": {"server": "server", "tool": "tool",
                "arguments": {"text": "x".repeat(limits::PERMISSION_COMMAND_BYTES)}}},
            "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]});
        assert!(
            public_question(
                "session/request_permission",
                &params,
                None,
                json!({}),
                &PaneNames::new()
            )
            .is_err()
        );
    }

    #[test]
    fn opencode_execute_permission_with_command_uses_command_not_bash_title() {
        let params = json!({"toolCall": {"kind": "execute", "title": "bash",
            "rawInput": {"command": "echo test", "description": "Print a test line"}},
            "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]});
        let (public, _) = public_question(
            "session/request_permission",
            &params,
            None,
            json!({}),
            &PaneNames::new(),
        )
        .expect("OpenCode command must be displayable");
        assert_eq!(public["presentation"]["command"], "echo test");
        assert!(!public.to_string().contains("Print a test line"));
    }

    #[test]
    fn execute_without_displayable_command_fails_closed() {
        let params = json!({"toolCall": {"kind": "execute", "title": "List directory",
            "content": [{"type": "content", "content": {"type": "text",
                "text": "List directory"}}]},
            "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]});
        assert!(
            public_question(
                "session/request_permission",
                &params,
                None,
                json!({}),
                &PaneNames::new()
            )
            .is_err()
        );

        let params = json!({"toolCall": {"kind": "execute", "title": "List directory",
            "rawInput": {"command": "  \n  "}},
            "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]});
        assert!(
            public_question(
                "session/request_permission",
                &params,
                None,
                json!({}),
                &PaneNames::new()
            )
            .is_err()
        );

        // OpenCode has emitted a pending bash permission with rawInput: {};
        // a title of "bash" cannot authorize an undisclosed command.
        let params = json!({"toolCall": {"kind": "execute", "title": "bash",
            "rawInput": {}},
            "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]});
        assert!(
            public_question(
                "session/request_permission",
                &params,
                None,
                json!({}),
                &PaneNames::new()
            )
            .is_err()
        );

        // A partial or malformed MCP shape is not a recognised tool request.
        for raw_input in [
            json!({"server": "soyeht-workspace"}),
            json!({"server": "soyeht-workspace", "tool": "message_agent", "arguments": "text"}),
        ] {
            let params = json!({"toolCall": {"kind": "execute", "title": "Run tool",
                "rawInput": raw_input},
                "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]});
            assert!(
                public_question(
                    "session/request_permission",
                    &params,
                    None,
                    json!({}),
                    &PaneNames::new()
                )
                .is_err()
            );
        }

        // An argv array is not a shell command string; never join it by guesswork.
        let params = json!({"toolCall": {"kind": "execute", "title": "Run command",
            "rawInput": {"command": ["ls", "-la"]}},
            "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]});
        assert!(
            public_question(
                "session/request_permission",
                &params,
                None,
                json!({}),
                &PaneNames::new()
            )
            .is_err()
        );
    }

    #[test]
    fn execute_command_over_display_limit_fails_closed() {
        let params = json!({"toolCall": {"kind": "execute", "title": "Run command",
            "rawInput": {"command": "x".repeat(limits::PERMISSION_COMMAND_BYTES + 1)}},
            "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]});
        assert!(
            public_question(
                "session/request_permission",
                &params,
                None,
                json!({}),
                &PaneNames::new()
            )
            .is_err()
        );
    }
}
