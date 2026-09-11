use super::{Changes, forms, now_ms, state::State};
use crate::{
    core::Core,
    errors::{self, Result},
    model::StoredPane,
};
use serde_json::{Value, json};
use soyeht_acp::client;
use soyeht_protocol::{
    ErrorKind, PaneState, PermissionMode, PermissionOption, PermissionOptionKind, Role,
};
use soyeht_store::agents::{self, NativeRequest};
use uuid::Uuid;

pub const QUESTION_TIMEOUT_MS: i64 = 30 * 60 * 1000;

fn invalid(message: &str) -> soyeht_protocol::RpcError {
    errors::error(ErrorKind::InvalidParams, message)
}

fn bounded(text: &str, maximum: usize) -> Result<String> {
    if text.chars().count() > maximum {
        return Err(invalid("Native question exceeds public protocol bounds"));
    }
    Ok(text.into())
}

pub fn receive(
    state: &mut State,
    saved: &mut StoredPane,
    source_seq: u64,
    native_id: Value,
    method: &str,
    params: Value,
    changes: &mut Changes,
) -> Result<()> {
    if params["sessionId"].as_str() != state.session_id.as_deref() {
        return Err(invalid("Question belongs to another session"));
    }
    let prompt = state
        .active
        .as_ref()
        .ok_or_else(|| invalid("Question has no active prompt"))?;
    let expires_ms = now_ms() + QUESTION_TIMEOUT_MS;
    let request_id = Uuid::new_v4().to_string();
    let expires_at = chrono::DateTime::from_timestamp_millis(expires_ms)
        .expect("bounded deadline")
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut public = json!({"paneId":saved.pane.pane_id,"requestId":request_id,
        "generation":saved.pane.generation,"promptId":prompt.prompt_id,"expiresAt":expires_at});
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
                    .any(|o| o.kind == PermissionOptionKind::AllowOnce),
                PermissionOptionKind::RejectAlways => options
                    .iter()
                    .any(|o| o.kind == PermissionOptionKind::RejectOnce),
                _ => true,
            })
            .collect();
        public["title"] = json!(bounded(
            params["toolCall"]["title"]
                .as_str()
                .unwrap_or("Permission required"),
            512
        )?);
        public["detail"] = json!(bounded(
            &serde_json::to_string(&params["toolCall"]).expect("tool JSON"),
            64 * 1024
        )?);
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
    if serde_json::to_vec(&public).expect("public question").len() > 512 * 1024 {
        return Err(invalid("Question exceeds snapshot item capacity"));
    }
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
    let request = changes.received.last().expect("received question").clone();
    if let Some(option) = automatic(state, &request, &saved.pane.cwd) {
        decision(state, &request, Some(&option), None, "rule", false, changes)?;
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

fn rule_key(state: &State, request: &NativeRequest) -> String {
    let mut tool = request.params["toolCall"].clone();
    if let Some(object) = tool.as_object_mut() {
        object.remove("toolCallId");
        object.remove("status");
    }
    use sha2::{Digest, Sha256};
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&json!([state.launch.id, tool])).expect("rule"))
    )
}

/// Permission policy never handles elicitation. Native persistent grants are
/// translated to one-time answers; their rule is owned by this pane alone.
pub fn automatic(state: &State, request: &NativeRequest, cwd: &str) -> Option<String> {
    if request.method != "session/request_permission" {
        return None;
    }
    let allowed = state
        .permission_rules
        .get(&rule_key(state, request))
        .copied()
        .or_else(|| {
            let edit = state.permission == PermissionMode::AcceptEdits
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
            if state.permission == PermissionMode::AllowAll || edit {
                Some(true)
            } else {
                None
            }
        })?;
    let kind = if allowed { "allow_once" } else { "reject_once" };
    request.public["options"]
        .as_array()?
        .iter()
        .find(|o| o["kind"] == kind)?
        .get("optionId")?
        .as_str()
        .map(str::to_owned)
}

pub fn decision(
    state: &mut State,
    request: &NativeRequest,
    option: Option<&str>,
    fields: Option<Value>,
    by: &str,
    expired: bool,
    changes: &mut Changes,
) -> Result<Value> {
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
            created_at: super::timestamp(),
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
                if state.permission_rules.len() >= 128
                    && !state
                        .permission_rules
                        .contains_key(&rule_key(state, request))
                {
                    return Err(invalid("Pane permission rule limit reached"));
                }
                state
                    .permission_rules
                    .insert(rule_key(state, request), allow);
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
        let pane = params["paneId"].as_str().expect("validated pane");
        let mut state = self.agent_state(pane)?;
        let mut saved = self.panes[pane].stored.clone();
        let request = agents::request(
            self.store.connection(),
            params["requestId"].as_str().expect("validated request"),
        )
        .map_err(errors::storage)?
        .filter(|r| r.pane_id == pane)
        .ok_or_else(|| errors::error(ErrorKind::NotFound, "Question not found"))?;
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
        let expired = request.expires_ms <= now_ms();
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
            option,
            fields,
            by,
            expired,
            &mut changes,
        )?;
        let other_open = agents::open_requests(self.store.connection(), pane)
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
