use super::{Changes, native_error, pending::UserOperation, responses, state::State};
use crate::{
    core::Core,
    errors::{self, Result},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use soyeht_protocol::{ConfigOperation, ConfigOperationKind, ConfigValue, PermissionMode};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyChange {
    pub permission: PermissionMode,
    pub yolo: bool,
    pub previous_permission: Option<PermissionMode>,
    pub previous_mode: Option<String>,
}

pub struct ChangeRequest<'a> {
    pub config: &'a str,
    pub value: ConfigValue,
    pub user: Option<UserOperation>,
    pub kind: ConfigOperationKind,
    pub started_at: String,
    pub now_ms: i64,
}

pub fn native_policy(value: &str) -> Option<PermissionMode> {
    match value {
        "default" | "ask" => Some(PermissionMode::Ask),
        "acceptEdits" | "accept_edits" => Some(PermissionMode::AcceptEdits),
        "bypassPermissions" | "allow_all" => Some(PermissionMode::AllowAll),
        _ => None,
    }
}

pub(super) fn require_native_policy(value: &str) -> Result<PermissionMode> {
    native_policy(value)
        .ok_or_else(|| errors::invalid("Agent reported an unsupported permission mode"))
}

pub fn observe_mode(state: &mut State, value: &str, explicit_selection: bool) -> Result<()> {
    let permission = require_native_policy(value)?;
    state.effective_mode = Some(value.into());
    state.permission = permission;
    if explicit_selection || state.permission != PermissionMode::AllowAll {
        state.yolo = false;
        state.previous_permission = None;
        state.previous_effective_mode = None;
    }
    Ok(())
}

pub fn begin(
    state: &mut State,
    pane: &str,
    request: ChangeRequest<'_>,
    changes: &mut Changes,
) -> Result<String> {
    let ChangeRequest {
        config,
        value,
        user,
        kind,
        started_at,
        now_ms,
    } = request;
    if state.operation.is_some() || state.cancel_at_ms.is_some() {
        return Err(errors::pane_busy("Another pane operation is pending"));
    }
    let session = state
        .session_id
        .as_deref()
        .ok_or_else(|| errors::pane_busy("Agent session is not ready"))?;
    let (method, params) = state
        .options
        .request(session, config, &value)
        .map_err(native_error)?;
    let id = Uuid::new_v4().to_string();
    state.operation = Some(ConfigOperation {
        operation_id: id.clone(),
        kind,
        config_id: config.into(),
        value: value.clone(),
        started_at,
    });
    state.operation_user = user;
    state.operation_timed_out = false;
    changes.events.push((
        "pane.config_changing".into(),
        json!({"paneId":pane,"configId":config,"value":value,"operationId":id}),
    ));
    responses::call(state, changes, method, params, &id, now_ms)?;
    Ok(id)
}

impl Core {
    pub fn configure_initial_agent(&mut self, pane: &str) -> Result<()> {
        let mut state = self.agent_state(pane)?;
        if state.operation.is_some() || state.initial_config_failed {
            return Ok(());
        }
        let Some((key, desired)) = state
            .requested_options
            .first_key_value()
            .map(|(k, v)| (k.clone(), v.clone()))
        else {
            return Ok(());
        };
        let category = match key.as_str() {
            "effort" => "thought_level",
            "permission" => "mode",
            _ => key.as_str(),
        };
        let option = state
            .options
            .values
            .iter()
            .find(|o| o.category.as_deref() == Some(category) || o.id == category)
            .cloned();
        let mut changes = Changes::default();
        if key == "permission" && option.is_none() {
            state.requested_options.remove(&key);
        } else {
            let selected = option.as_ref().and_then(|option| {
                if key == "permission" {
                    option
                        .options
                        .iter()
                        .find(|choice| {
                            native_policy(&choice.value).as_ref() == Some(&state.permission)
                        })
                        .map(|choice| ConfigValue::V0(choice.value.clone()))
                } else {
                    Some(desired.clone())
                }
            });
            if let Some((option, value)) = option.zip(selected) {
                if option.current_value == value {
                    if key == "permission"
                        && let ConfigValue::V0(mode) = &value
                    {
                        observe_mode(&mut state, mode, false)?;
                    }
                    state.requested_options.remove(&key);
                } else {
                    match begin(
                        &mut state,
                        pane,
                        ChangeRequest {
                            config: &option.id,
                            value,
                            user: None,
                            kind: ConfigOperationKind::Config,
                            started_at: self.timestamp(),
                            now_ms: self.now_ms(),
                        },
                        &mut changes,
                    ) {
                        Ok(_) => state.initial_option = Some(key.clone()),
                        Err(error) => {
                            state.initial_config_failed = true;
                            changes.events.push(("pane.config_failed".into(),json!({"paneId":pane,"configId":option.id,"value":desired,"reason":error.to_string()})));
                        }
                    }
                }
            } else if key == "permission" {
                // No recognized native preset: enforce the engine's question
                // policy without inventing an undocumented configuration value.
                state.requested_options.remove(&key);
            } else {
                state.initial_config_failed = true;
                changes.events.push(("pane.config_failed".into(),json!({"paneId":pane,"configId":key,"value":desired,"reason":"Requested selector is not offered by this session"})));
            }
        }
        self.commit_agent(
            self.panes[pane].stored.clone(),
            state,
            changes,
            None,
            json!({}),
        )?;
        Ok(())
    }

    pub fn agent_yolo(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let pane = errors::param_str(params, "paneId")?;
        let mut state = self.agent_state(pane)?;
        if state.operation.is_some() || state.cancel_at_ms.is_some() {
            return Err(errors::pane_busy("Another pane operation is pending"));
        }
        let on = errors::param_bool(params, "on")?;
        if on == state.yolo {
            let result = json!({"effectiveMode":state.permission,"on":state.yolo});
            return Ok(self
                .commit_agent(
                    self.panes[pane].stored.clone(),
                    state,
                    Changes::default(),
                    Some((connection, "pane.yolo", params)),
                    result,
                )?
                .result);
        }
        let desired = if on {
            PermissionMode::AllowAll
        } else {
            state
                .previous_permission
                .clone()
                .unwrap_or(PermissionMode::Ask)
        };
        let option = state
            .options
            .values
            .iter()
            .find(|o| o.category.as_deref() == Some("mode") || o.id == "mode")
            .cloned();
        let policy = PolicyChange {
            permission: desired.clone(),
            yolo: on,
            previous_permission: if on {
                Some(state.permission.clone())
            } else {
                None
            },
            previous_mode: if on {
                option.as_ref().and_then(|o| match &o.current_value {
                    ConfigValue::V0(v) => Some(v.clone()),
                    _ => None,
                })
            } else {
                None
            },
        };
        let selection = option.as_ref().and_then(|option| {
            if !on && let Some(previous) = &state.previous_effective_mode {
                return option
                    .options
                    .iter()
                    .find(|choice| &choice.value == previous)
                    .map(|choice| choice.value.clone());
            }
            option
                .options
                .iter()
                .find(|choice| native_policy(&choice.value).as_ref() == Some(&desired))
                .map(|choice| choice.value.clone())
        });
        let mut changes = Changes::default();
        let result = if let Some(selection) = selection {
            let user = UserOperation {
                scope: self.clients[connection].scope.clone(),
                key: errors::param_str(params, "idempotencyKey")?.into(),
            };
            let native_option = errors::invariant(
                option.as_ref(),
                "Selected native mode has no configuration option",
            )?;
            let operation = begin(
                &mut state,
                pane,
                ChangeRequest {
                    config: &native_option.id,
                    value: ConfigValue::V0(selection),
                    user: Some(user),
                    kind: ConfigOperationKind::Yolo,
                    started_at: self.timestamp(),
                    now_ms: self.now_ms(),
                },
                &mut changes,
            )?;
            state.pending_policy = Some(policy);
            json!({"_nativePending":operation})
        } else {
            if !on && state.previous_effective_mode.is_some() {
                return Err(errors::invalid(
                    "The previous native mode is no longer offered",
                ));
            }
            state.permission = policy.permission;
            state.yolo = policy.yolo;
            state.previous_permission = policy.previous_permission;
            state.previous_effective_mode = policy.previous_mode;
            json!({"effectiveMode":state.permission,"on":state.yolo})
        };
        Ok(self
            .commit_agent(
                self.panes[pane].stored.clone(),
                state,
                changes,
                Some((connection, "pane.yolo", params)),
                result,
            )?
            .result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::ErrorKind;
    use soyeht_protocol::HarnessLaunch;

    #[test]
    fn unsupported_native_mode_is_rejected_without_changing_policy() {
        let mut state = State::new(
            HarnessLaunch {
                id: "test".into(),
                name: "Test".into(),
                command: "/bin/false".into(),
                args: vec![],
                version: "test".into(),
            },
            "digest".into(),
            PermissionMode::AllowAll,
        );
        state.effective_mode = Some("bypassPermissions".into());
        state.yolo = true;
        state.previous_permission = Some(PermissionMode::Ask);
        state.previous_effective_mode = Some("default".into());

        let error = observe_mode(&mut state, "unexpected", false).unwrap_err();

        assert_eq!(errors::kind(&error), ErrorKind::InvalidParams);
        assert_eq!(state.permission, PermissionMode::AllowAll);
        assert_eq!(state.effective_mode.as_deref(), Some("bypassPermissions"));
        assert!(state.yolo);
        assert_eq!(state.previous_permission, Some(PermissionMode::Ask));
        assert_eq!(state.previous_effective_mode.as_deref(), Some("default"));
    }
}
