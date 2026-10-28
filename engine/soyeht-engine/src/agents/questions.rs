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
    PermissionPresentation, PermissionToolArgument, Role, event, limits, method,
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
        event::PANE_PERMISSION_REQUESTED
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
        event::PANE_ELICITATION_REQUESTED
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

/// Closes every open request of a pane as the system: no option, no fields, not expired.
pub fn decide_open_as_system(
    store: &impl StorePort,
    pane: &str,
    state: &mut State,
    changes: &mut Changes,
    timestamp: &str,
) -> Result<()> {
    for request in store.agent_open_requests(pane).map_err(errors::storage)? {
        decision(
            state,
            &request,
            Decision {
                option: None,
                fields: None,
                by: "system",
                expired: false,
                timestamp,
            },
            changes,
        )?;
    }
    Ok(())
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
            event::PANE_PERMISSION_ANSWERED
        } else {
            event::PANE_ELICITATION_ANSWERED
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
        let expected = if method == method::PANE_PERMISSION_ANSWER {
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
mod presentation_tests;
