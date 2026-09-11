use super::{
    Changes, native_error,
    state::{Phase, State},
    transcript,
};
use crate::{
    errors::{self, Result},
    model::StoredPane,
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_acp::{
    Message, client,
    config::Options,
    session::{self, Initialization},
};
use soyeht_protocol::{
    ErrorKind, PaneState, ProbeCapability, ProbeResult, TranscriptKind, TranscriptRole,
};

pub const PROBED_METHODS: [&str; 3] = ["session/list", "session/load", "session/set_config_option"];

pub fn call(
    state: &mut State,
    changes: &mut Changes,
    method: &str,
    params: Value,
    context: &str,
    now_ms: i64,
) -> Result<()> {
    changes.outgoing.push(
        state
            .client
            .request(method, params, context, now_ms as u64)
            .map_err(native_error)?,
    );
    Ok(())
}

fn open(state: &mut State, saved: &StoredPane, changes: &mut Changes, now_ms: i64) -> Result<()> {
    if let Some(id) = state.session_id.clone() {
        state.phase = Phase::Loading;
        changes.clear_replay = true;
        state.replay_role = None;
        state.open_messages.clear();
        call(
            state,
            changes,
            "session/load",
            client::load_session(&saved.pane.cwd, &id),
            "load",
            now_ms,
        )
    } else {
        state.phase = Phase::Opening;
        call(
            state,
            changes,
            "session/new",
            client::new_session(&saved.pane.cwd),
            "open",
            now_ms,
        )
    }
}

fn needs_login(state: &mut State, saved: &mut StoredPane, changes: &mut Changes) {
    state.phase = Phase::NeedsLogin;
    state.probe_started_ms = None;
    saved.pane.state = PaneState::Starting;
    saved.pane.phrase = "Sign-in required".into();
    let methods = state
        .initialized
        .as_ref()
        .map(|i| {
            i.auth_methods
                .iter()
                .map(|m| m.public.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    changes.events.push((
        "pane.needs_login".into(),
        json!({"paneId":saved.pane.pane_id,"harness":state.launch.id,"authMethods":methods}),
    ));
}

fn finish_configuration(
    state: &mut State,
    saved: &StoredPane,
    operation: soyeht_protocol::ConfigOperation,
    native_result: std::result::Result<Value, soyeht_acp::RpcError>,
    changes: &mut Changes,
) -> Result<()> {
    let confirmation = native_result
        .map_err(|error| {
            errors::error(
                if error.code == -32602 {
                    ErrorKind::InvalidParams
                } else {
                    ErrorKind::HarnessUnavailable
                },
                error.message,
            )
        })
        .and_then(|value| {
            state
                .options
                .confirmed(&operation.config_id, &operation.value, &value)
                .map_err(native_error)
        });
    let result = match confirmation {
        Ok(options) => {
            state.options = options;
            if let Some(key) = state.initial_option.take() {
                state.requested_options.remove(&key);
            }
            if let Some(option) = state
                .options
                .values
                .iter()
                .find(|option| option.id == operation.config_id)
            {
                let key = match option.category.as_deref().unwrap_or(&option.id) {
                    "thought_level" => "effort",
                    "mode" => "permission",
                    other => other,
                };
                state.requested_options.remove(key);
            }
            state.initial_config_failed = false;
            if let Some(option) = state.options.values.iter().find(|option| {
                option.id == operation.config_id
                    && (option.category.as_deref() == Some("mode") || option.id == "mode")
            }) && let soyeht_protocol::ConfigValue::V0(value) = &option.current_value
            {
                let value = value.clone();
                super::configuration::observe_mode(state, &value, state.pending_policy.is_none())?;
            }
            if let Some(policy) = state.pending_policy.take() {
                state.permission = policy.permission;
                state.yolo = policy.yolo;
                state.previous_permission = policy.previous_permission;
                state.previous_effective_mode = policy.previous_mode;
            }
            changes.events.push(("pane.config_changed".into(),json!({"paneId":saved.pane.pane_id,"configId":operation.config_id,"value":operation.value,"options":state.options.values,"sessionPreserved":true})));
            Ok(
                if operation.kind == soyeht_protocol::ConfigOperationKind::Yolo {
                    json!({"effectiveMode":state.permission,"on":state.yolo})
                } else {
                    json!({"applied":true,"effective":operation.value,"sessionPreserved":true})
                },
            )
        }
        Err(error) => {
            if state.initial_option.take().is_some() {
                state.initial_config_failed = true;
            }
            state.pending_policy = None;
            changes.events.push(("pane.config_failed".into(),json!({"paneId":saved.pane.pane_id,"configId":operation.config_id,"value":operation.value,"reason":error.message})));
            Err(error)
        }
    };
    if let Some(user) = state.operation_user.take() {
        changes.completed_user = Some(super::pending::Completion { user, result });
    }
    state.operation = None;
    Ok(())
}

fn initialized_version(state: &State) -> Result<&str> {
    Ok(&errors::invariant(
        state.initialized.as_ref(),
        "Agent session opened before initialization completed",
    )?
    .version)
}

fn fingerprint(state: &State, version: &str) -> Result<String> {
    session::fingerprint(&state.binary_digest, version, &json!(state.launch)).map_err(native_error)
}

pub fn response(
    store: &impl StorePort,
    state: &mut State,
    saved: &mut StoredPane,
    message: Message,
    changes: &mut Changes,
    now_ms: i64,
    timestamp: &str,
) -> Result<()> {
    let (request, result) = state.client.response(message).map_err(native_error)?;
    if request.context == "auth" {
        let ok = result.is_ok();
        state.phase = if ok {
            Phase::RestartAfterProbe
        } else {
            Phase::NeedsLogin
        };
        state.probe_started_ms = None;
        changes.clear_failed_probes = ok;
        changes.events.push(("pane.auth_finished".into(),json!({"paneId":saved.pane.pane_id,"ok":ok,"reason":result.err().map(|e|e.message).unwrap_or_else(||"Sign-in completed".into())})));
        return Ok(());
    }
    if request.context == "initialize" {
        let value = result.map_err(|e| errors::error(ErrorKind::HarnessUnavailable, e.message))?;
        let info = Initialization::parse(&value).map_err(native_error)?;
        state.fingerprint = fingerprint(state, &info.version)?;
        state.initialized = Some(info);
        let mut all_known = true;
        for method in PROBED_METHODS {
            all_known &= store
                .agent_probe(&state.fingerprint, method)
                .map_err(errors::storage)?
                .is_some();
        }
        if all_known {
            return open(state, saved, changes, now_ms);
        }
        state.phase = Phase::ProbeNew;
        state.probe_started_ms = Some(now_ms);
        return call(
            state,
            changes,
            "session/new",
            client::new_session(&saved.pane.cwd),
            "probe:new",
            now_ms,
        );
    }
    if request.context == "probe:new" {
        match result {
            Ok(value) => {
                state.probe_session_id = Some(session::session_id(&value).map_err(native_error)?);
                state.options = Options::from_response(&value).map_err(native_error)?;
                state.phase = Phase::ProbeList;
                return call(
                    state,
                    changes,
                    "session/list",
                    json!({}),
                    "probe:list",
                    now_ms,
                );
            }
            Err(error) if error.code == -32000 => {
                needs_login(state, saved, changes);
                return Ok(());
            }
            Err(error) => return Err(errors::error(ErrorKind::HarnessUnavailable, error.message)),
        }
    }
    if request.context.starts_with("probe:") {
        let mut classified = session::probe_result(result.as_ref());
        if request.context == "probe:config"
            && let Ok(value) = &result
        {
            let confirmed = request.params["configId"]
                .as_str()
                .zip(
                    serde_json::from_value::<soyeht_protocol::ConfigValue>(
                        request.params["value"].clone(),
                    )
                    .ok(),
                )
                .is_some_and(|(id, desired)| state.options.confirmed(id, &desired, value).is_ok());
            if !confirmed {
                classified = ProbeResult::Inconclusive;
            }
        }
        changes.probes.push(ProbeCapability {
            method: request.method.clone(),
            result: classified,
            checked_at: timestamp.into(),
        });
        let id = state.probe_session_id.clone().ok_or_else(|| {
            errors::error(ErrorKind::InternalError, "Probe has no disposable session")
        })?;
        match request.context.as_str() {
            "probe:list" => {
                state.phase = Phase::ProbeLoad;
                call(
                    state,
                    changes,
                    "session/load",
                    client::load_session(&saved.pane.cwd, &id),
                    "probe:load",
                    now_ms,
                )?;
            }
            "probe:load" => {
                if let Some(option) = state
                    .options
                    .values
                    .iter()
                    .find(|o| {
                        state.options.routes.get(&o.id)
                            == Some(&soyeht_acp::config::Route::ConfigOption)
                    })
                    .cloned()
                {
                    state.phase = Phase::ProbeConfig;
                    let (method, params) = state
                        .options
                        .request(&id, &option.id, &option.current_value)
                        .map_err(native_error)?;
                    call(state, changes, method, params, "probe:config", now_ms)?;
                } else {
                    // No advertised option supplies valid probe parameters.
                    // This is inconclusive, never evidence of a missing method.
                    changes.probes.push(ProbeCapability {
                        method: "session/set_config_option".into(),
                        result: ProbeResult::Inconclusive,
                        checked_at: timestamp.into(),
                    });
                    state.phase = Phase::RestartAfterProbe;
                }
            }
            "probe:config" => state.phase = Phase::RestartAfterProbe,
            _ => {
                return Err(errors::error(
                    ErrorKind::InternalError,
                    "Unknown probe response",
                ));
            }
        }
        return Ok(());
    }
    if request.context == "open" || request.context == "load" {
        match result {
            Ok(value) => {
                if request.context == "open" {
                    let id = session::session_id(&value).map_err(native_error)?;
                    if id.chars().count() > 128 {
                        return Err(errors::error(
                            ErrorKind::InvalidParams,
                            "Native session ID exceeds the public protocol bound",
                        ));
                    }
                    state.session_id = Some(id);
                }
                if request.context == "load" {
                    changes.replay_messages = true;
                    transcript::complete(store, &saved.pane.pane_id, state, changes)?;
                    changes.publish_replay = true;
                    state.replay_role = None;
                }
                if value.get("configOptions").is_some()
                    || value.get("modes").is_some()
                    || request.context == "open"
                {
                    state.options = Options::from_response(&value).map_err(native_error)?;
                }
                state.phase = Phase::Ready;
                saved.pane.state = if state.has_finished_turn {
                    PaneState::Idle
                } else {
                    PaneState::Starting
                };
                saved.pane.phrase = "Agent session ready".into();
                changes.events.push(("pane.session_started".into(),json!({"paneId":saved.pane.pane_id,"harness":state.launch.id,"version":initialized_version(state)?,"sessionId":state.session_id,"generation":saved.pane.generation,"options":state.options.values})));
            }
            Err(error) if error.code == -32000 => needs_login(state, saved, changes),
            Err(error) if request.context == "load" && matches!(error.code, -32601 | -32002) => {
                changes.clear_replay = true;
                state.open_messages.clear();
                state.replay_role = None;
                let text =
                    "Native session lost; opening a new session without a continuation package";
                changes.events.push(("pane.session_lost".into(),json!({"paneId":saved.pane.pane_id,"reason":error.message,"recovery":"new_session"})));
                changes.messages.push(transcript::new_message(
                    state,
                    saved.pane.generation,
                    TranscriptRole::System,
                    TranscriptKind::System,
                    text.into(),
                    true,
                )?);
                state.session_id = None;
                state.has_finished_turn = false;
                open(state, saved, changes, now_ms)?;
            }
            Err(error) => return Err(errors::error(ErrorKind::HarnessUnavailable, error.message)),
        }
        return Ok(());
    }
    if request.method == "session/prompt" {
        if state.active.as_ref().map(|p| p.prompt_id.as_str()) != Some(request.context.as_str()) {
            return Err(errors::error(
                ErrorKind::InternalError,
                "Prompt response belongs to another turn",
            ));
        }
        let reason = match result {
            Ok(value) => session::stop_reason(&value)
                .map_err(native_error)?
                .to_owned(),
            Err(error) => {
                changes.messages.push(transcript::new_message(
                    state,
                    saved.pane.generation,
                    TranscriptRole::System,
                    TranscriptKind::System,
                    error.message,
                    true,
                )?);
                "error".into()
            }
        };
        for request in store
            .agent_open_requests(&saved.pane.pane_id)
            .map_err(errors::storage)?
        {
            super::questions::decision(
                state,
                &request,
                super::questions::Decision {
                    option: None,
                    fields: None,
                    by: "system",
                    expired: false,
                    timestamp,
                },
                changes,
            )?;
        }
        transcript::complete(store, &saved.pane.pane_id, state, changes)?;
        state.active = None;
        state.cancel_at_ms = None;
        state.has_finished_turn = true;
        saved.pane.state = PaneState::Idle;
        saved.pane.phrase = "Agent turn ended".into();
        changes.events.push((
            "pane.turn_ended".into(),
            json!({"paneId":saved.pane.pane_id,"promptId":request.context,"stopReason":reason}),
        ));
        return Ok(());
    }
    if let Some(operation) = state
        .operation
        .clone()
        .filter(|operation| operation.operation_id == request.context)
    {
        finish_configuration(state, saved, operation, result, changes)?;
        return Ok(());
    }
    Err(errors::error(
        ErrorKind::InvalidParams,
        "Unexpected ACP response context",
    ))
}
