use super::{
    Changes, native_error,
    state::{Phase, State},
    transcript,
};
use crate::{
    errors::{self, Result},
    model::StoredPane,
    ports::StorePort,
    tasks::ports::TaskStorePort,
};
use serde_json::{Value, json};
use soyeht_acp::{
    Message, client,
    config::Options,
    session::{self, Initialization},
};
use soyeht_protocol::{PaneState, ProbeCapability, ProbeResult, TranscriptKind, TranscriptRole};
use soyeht_store::agents::AgentModelRefusal;
use std::path::Path;

pub const PROBED_METHODS: [&str; 3] = ["session/list", "session/load", "session/set_config_option"];

pub struct Runtime<'runtime> {
    pub data_directory: &'runtime Path,
    pub now_ms: i64,
    pub timestamp: &'runtime str,
}

pub fn call(
    state: &mut State,
    changes: &mut Changes,
    method: &str,
    params: Value,
    context: &str,
    now_ms: i64,
) -> Result<()> {
    if method == "session/prompt" {
        state.active_output_observed = false;
        state.active_silent_since_ms = Some(now_ms);
    }
    changes.outgoing.push(
        state
            .client
            .request(method, params, context, now_ms as u64)
            .map_err(native_error)?,
    );
    Ok(())
}

fn open(
    store: &(impl StorePort + TaskStorePort),
    runtime: &Runtime<'_>,
    state: &mut State,
    saved: &StoredPane,
    changes: &mut Changes,
) -> Result<()> {
    let servers = if state.mcp_enabled {
        vec![super::mcp::declaration(
            store,
            runtime.data_directory,
            state,
            saved,
        )?]
    } else {
        Vec::new()
    };
    // A new harness process cannot report the previous one's subagents.
    super::subagents::disconnect_all(store, &saved.pane.pane_id, state, changes)?;
    if let Some(id) = state.session_id.clone() {
        state.phase = Phase::Loading;
        changes.clear_replay = true;
        state.replay_role = None;
        state.replay_user = None;
        state.replay_continuation_turn = false;
        state.open_messages.clear();
        // A switched session starts at the switch, so its marker leads the
        // replay. The replayed continuation prompt itself is suppressed.
        if state.internal_continuation.is_some() {
            changes.replay_messages = true;
            let marker = transcript::handover(state, saved.pane.generation)?;
            changes.messages.push(marker);
        }
        call(
            state,
            changes,
            "session/load",
            super::opening::parameters(
                runtime.data_directory,
                state,
                client::load_session_with_mcp(&saved.pane.cwd, &id, &servers),
            )?,
            "load",
            runtime.now_ms,
        )
    } else {
        // A hook capability belongs to one native session. Rotating before
        // session/new makes the prior session's command unusable immediately,
        // including the recovery path after session/load reports it missing.
        state.rotate_hook_binding()?;
        state.phase = Phase::Opening;
        call(
            state,
            changes,
            "session/new",
            super::opening::parameters(
                runtime.data_directory,
                state,
                client::new_session_with_mcp(&saved.pane.cwd, &servers),
            )?,
            "open",
            runtime.now_ms,
        )
    }
}

fn needs_login(state: &mut State, saved: &mut StoredPane, changes: &mut Changes) {
    state.phase = Phase::NeedsLogin;
    state.auth_required = true;
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
            if error.code == -32602 {
                errors::invalid(error.message)
            } else {
                errors::harness_unavailable(error.message)
            }
        })
        .and_then(|value| {
            state
                .options
                .confirmed(&operation.config_id, &operation.value, &value)
                .map_err(native_error)
        });
    let result = match confirmation {
        Ok(options) => {
            if let Some(key) = state.initial_option.take() {
                state.requested_options.remove(&key);
            }
            super::configuration::confirmed(state, options, &operation.config_id);
            state.initial_config_failed = false;
            changes.events.push(("pane.config_changed".into(),json!({"paneId":saved.pane.pane_id,"configId":operation.config_id,"value":operation.value,"options":state.options.values,"sessionPreserved":true})));
            Ok(super::configuration::applied(&operation.value))
        }
        Err(error) => {
            match state.initial_option.take() {
                Some(key) if key == super::configuration::MODE_KEY => {
                    super::configuration::refused_mode(
                        state,
                        saved.pane.generation,
                        &operation.value,
                        changes,
                    )?;
                }
                Some(_) => state.initial_config_failed = true,
                None => {}
            }
            changes.events.push(("pane.config_failed".into(),json!({"paneId":saved.pane.pane_id,"configId":operation.config_id,"value":operation.value,"reason":error.to_string()})));
            Err(error)
        }
    };
    if let Some(user) = state.operation_user.take() {
        changes.completed_user = Some(super::pending::Completion {
            user,
            result: result.map_err(errors::wire),
        });
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

type NativeResult = std::result::Result<Value, soyeht_acp::RpcError>;

fn clean_agent_error(message: &str) -> String {
    const INTERNAL_PREFIX: &str = "Internal error:";
    message
        .get(..INTERNAL_PREFIX.len())
        .filter(|prefix| prefix.eq_ignore_ascii_case(INTERNAL_PREFIX))
        .and_then(|_| message.get(INTERNAL_PREFIX.len()..))
        .unwrap_or(message)
        .trim()
        .to_owned()
}

fn assistant_already_reported_error(
    store: &impl StorePort,
    pane: &str,
    state: &State,
    error: &str,
) -> Result<bool> {
    for (key, seq) in &state.open_messages {
        if (key == "message" || key.starts_with("message:"))
            && let Some(message) = transcript::message(store, pane, *seq)?
            && message.text.trim() == error
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn selected_model(state: &State) -> Option<&str> {
    state
        .options
        .values
        .iter()
        .find(|option| option.category.as_deref() == Some("model") || option.id == "model")
        .and_then(|option| match &option.current_value {
            soyeht_protocol::ConfigValue::V0(value) => Some(value.as_str()),
            _ => None,
        })
}

fn observed_model_refusal(code: i32, reason: &str) -> bool {
    if code != -32603 {
        return false;
    }
    let reason = reason.to_ascii_lowercase();
    reason.contains("model is disabled")
        || (reason.contains("package has expired") && reason.contains("unavailable"))
}

fn bounded_refusal_reason(reason: &str) -> Option<String> {
    if reason.is_empty() {
        return None;
    }
    let mut end = reason
        .len()
        .min(soyeht_protocol::limits::MODEL_REFUSAL_REASON_BYTES);
    while !reason.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let reason = reason.get(..end)?.trim_end();
    (!reason.is_empty()).then(|| reason.to_owned())
}

fn auth_response(
    state: &mut State,
    saved: &StoredPane,
    changes: &mut Changes,
    result: NativeResult,
) {
    let ok = result.is_ok();
    state.auth_required = !ok;
    state.phase = if ok {
        Phase::RestartAfterProbe
    } else {
        Phase::NeedsLogin
    };
    state.probe_started_ms = None;
    changes.clear_failed_probes = ok;
    changes.events.push(("pane.auth_finished".into(),json!({"paneId":saved.pane.pane_id,"ok":ok,"reason":result.err().map(|error|error.message).unwrap_or_else(||"Sign-in completed".into())})));
}

fn initialize_response(
    store: &(impl StorePort + TaskStorePort),
    runtime: &Runtime<'_>,
    state: &mut State,
    saved: &StoredPane,
    changes: &mut Changes,
    result: NativeResult,
) -> Result<()> {
    let value = result.map_err(|error| errors::harness_unavailable(error.message))?;
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
        return open(store, runtime, state, saved, changes);
    }
    state.phase = Phase::ProbeNew;
    state.probe_started_ms = Some(runtime.now_ms);
    call(
        state,
        changes,
        "session/new",
        client::new_session(&saved.pane.cwd),
        "probe:new",
        runtime.now_ms,
    )
}

fn probe_new_response(
    state: &mut State,
    saved: &mut StoredPane,
    changes: &mut Changes,
    result: NativeResult,
    now_ms: i64,
) -> Result<()> {
    match result {
        Ok(value) => {
            state.probe_session_id = Some(session::session_id(&value).map_err(native_error)?);
            state.options = Options::from_response(&value).map_err(native_error)?;
            state.phase = Phase::ProbeList;
            call(
                state,
                changes,
                "session/list",
                json!({}),
                "probe:list",
                now_ms,
            )
        }
        Err(error) if soyeht_acp::rpc::is_auth_required(&error) => {
            needs_login(state, saved, changes);
            Ok(())
        }
        Err(error) => Err(errors::harness_unavailable(error.message)),
    }
}

fn probe_response(
    state: &mut State,
    saved: &StoredPane,
    request: &client::Call,
    changes: &mut Changes,
    result: NativeResult,
    now_ms: i64,
    timestamp: &str,
) -> Result<()> {
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
    let id = state
        .probe_session_id
        .clone()
        .ok_or_else(|| errors::internal("Probe has no disposable session"))?;
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
            )
        }
        "probe:load" => probe_configuration(state, changes, &id, now_ms, timestamp),
        "probe:config" => {
            state.phase = Phase::RestartAfterProbe;
            Ok(())
        }
        _ => Err(errors::internal("Unknown probe response")),
    }
}

fn probe_configuration(
    state: &mut State,
    changes: &mut Changes,
    session_id: &str,
    now_ms: i64,
    timestamp: &str,
) -> Result<()> {
    let option = state
        .options
        .values
        .iter()
        .find(|option| {
            state.options.routes.get(&option.id) == Some(&soyeht_acp::config::Route::ConfigOption)
        })
        .cloned();
    if let Some(option) = option {
        state.phase = Phase::ProbeConfig;
        let (method, params) = state
            .options
            .request(session_id, &option.id, &option.current_value)
            .map_err(native_error)?
            .ok_or_else(|| errors::internal("A native option has no native request"))?;
        call(state, changes, method, params, "probe:config", now_ms)
    } else {
        // No advertised option supplies valid probe parameters. This is
        // inconclusive, never evidence of a missing method.
        changes.probes.push(ProbeCapability {
            method: "session/set_config_option".into(),
            result: ProbeResult::Inconclusive,
            checked_at: timestamp.into(),
        });
        state.phase = Phase::RestartAfterProbe;
        Ok(())
    }
}

fn session_response(
    store: &(impl StorePort + TaskStorePort),
    runtime: &Runtime<'_>,
    state: &mut State,
    saved: &mut StoredPane,
    context: &str,
    changes: &mut Changes,
    result: NativeResult,
) -> Result<()> {
    match result {
        Ok(value) => session_opened(store, state, saved, context, changes, &value),
        Err(error) if soyeht_acp::rpc::is_auth_required(&error) => {
            needs_login(state, saved, changes);
            Ok(())
        }
        Err(error) if context == "load" && matches!(error.code, -32601 | -32002) => {
            session_lost(store, runtime, state, saved, changes, error.message)
        }
        Err(error) => Err(errors::harness_unavailable(error.message)),
    }
}

fn session_opened(
    store: &impl StorePort,
    state: &mut State,
    saved: &mut StoredPane,
    context: &str,
    changes: &mut Changes,
    value: &Value,
) -> Result<()> {
    if context == "open" {
        let id = session::session_id(value).map_err(native_error)?;
        if id.chars().count() > 128 {
            return Err(errors::invalid(
                "Native session ID exceeds the public protocol bound",
            ));
        }
        state.session_id = Some(id);
    }
    if context == "load" {
        changes.replay_messages = true;
        super::updates::finish_replayed_user(
            store,
            &saved.pane.pane_id,
            saved.pane.generation,
            state,
            changes,
        )?;
        transcript::complete(store, &saved.pane.pane_id, state, changes)?;
        changes.publish_replay = true;
        state.replay_role = None;
    }
    if value.get("configOptions").is_some() || value.get("modes").is_some() || context == "open" {
        super::configuration::open_session(
            state,
            Options::from_response(value).map_err(native_error)?,
        )?;
    }
    state.phase = Phase::Ready;
    saved.pane.state = if state.has_finished_turn {
        PaneState::Idle
    } else {
        PaneState::Starting
    };
    saved.pane.phrase = "Agent session ready".into();
    changes.events.push(("pane.session_started".into(),json!({"paneId":saved.pane.pane_id,"harness":state.launch.id,"version":initialized_version(state)?,"sessionId":state.session_id,"generation":saved.pane.generation,"options":state.options.values})));
    Ok(())
}

fn session_lost(
    store: &(impl StorePort + TaskStorePort),
    runtime: &Runtime<'_>,
    state: &mut State,
    saved: &StoredPane,
    changes: &mut Changes,
    reason: String,
) -> Result<()> {
    changes.clear_replay = true;
    state.open_messages.clear();
    // The discarded replay may have reported subagents; none of them exist.
    state.subagents.clear();
    state.replay_role = None;
    let text = "Native session lost; opening a new session without a continuation package";
    changes.events.push((
        "pane.session_lost".into(),
        json!({"paneId":saved.pane.pane_id,"reason":reason,"recovery":"new_session"}),
    ));
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
    open(store, runtime, state, saved, changes)
}

fn prompt_response(
    store: &impl StorePort,
    state: &mut State,
    saved: &mut StoredPane,
    context: &str,
    changes: &mut Changes,
    result: NativeResult,
    timestamp: &str,
) -> Result<()> {
    if state
        .active
        .as_ref()
        .map(|prompt| prompt.prompt_id.as_str())
        != Some(context)
    {
        return Err(errors::internal("Prompt response belongs to another turn"));
    }
    let cancel_requested = state.cancel_at_ms.is_some();
    let authentication_required = matches!(
        &result,
        Err(error) if soyeht_acp::rpc::is_auth_required(error)
    );
    let reason = match result {
        Ok(value) => session::stop_reason(&value)
            .map_err(native_error)?
            .to_owned(),
        Err(error) => {
            let clean = clean_agent_error(&error.message);
            let model = selected_model(state).map(str::to_owned);
            let refusal = model
                .as_ref()
                .filter(|_| observed_model_refusal(error.code, &clean));
            let text = if let Some(model) = refusal {
                if let Some(reason) = bounded_refusal_reason(&clean) {
                    changes.model_refusal = Some(AgentModelRefusal {
                        harness_id: state.launch.id.clone(),
                        model_id: model.clone(),
                        reason,
                        observed_at: timestamp.to_owned(),
                    });
                }
                format!("The agent refused model \"{model}\": {clean}")
            } else {
                format!("Agent error: {clean}")
            };
            // Some adapters emit the error as an assistant chunk before returning
            // the same typed RPC error. Keep one visible copy for this turn.
            if !assistant_already_reported_error(store, &saved.pane.pane_id, state, &clean)? {
                changes.messages.push(transcript::new_message(
                    state,
                    saved.pane.generation,
                    TranscriptRole::System,
                    TranscriptKind::System,
                    text,
                    true,
                )?);
            }
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
    if cancel_requested && reason == "cancelled" {
        changes.messages.push(transcript::new_message(
            state,
            saved.pane.generation,
            TranscriptRole::System,
            TranscriptKind::System,
            "Turn cancelled by the user.".into(),
            true,
        )?);
    }
    state.active = None;
    state.active_output_observed = true;
    state.active_silent_since_ms = None;
    state.cancel_at_ms = None;
    state.has_finished_turn = true;
    if !matches!(reason.as_str(), "error" | "cancelled") {
        state.auth_required = false;
    }
    saved.pane.state = PaneState::Idle;
    saved.pane.phrase = "Agent turn ended".into();
    changes.events.push((
        "pane.turn_ended".into(),
        json!({"paneId":saved.pane.pane_id,"promptId":context,"stopReason":reason}),
    ));
    if authentication_required {
        needs_login(state, saved, changes);
    }
    Ok(())
}

pub fn response(
    store: &(impl StorePort + TaskStorePort),
    runtime: &Runtime<'_>,
    state: &mut State,
    saved: &mut StoredPane,
    message: Message,
    changes: &mut Changes,
) -> Result<()> {
    let (request, result) = state.client.response(message).map_err(native_error)?;
    if request.context == "auth" {
        auth_response(state, saved, changes, result);
        return Ok(());
    }
    if request.context == "initialize" {
        return initialize_response(store, runtime, state, saved, changes, result);
    }
    if request.context == "probe:new" {
        return probe_new_response(state, saved, changes, result, runtime.now_ms);
    }
    if request.context.starts_with("probe:") {
        return probe_response(
            state,
            saved,
            &request,
            changes,
            result,
            runtime.now_ms,
            runtime.timestamp,
        );
    }
    if request.context == "open" || request.context == "load" {
        return session_response(
            store,
            runtime,
            state,
            saved,
            &request.context,
            changes,
            result,
        );
    }
    if request.method == "session/prompt" {
        return prompt_response(
            store,
            state,
            saved,
            &request.context,
            changes,
            result,
            runtime.timestamp,
        );
    }
    if let Some(operation) = state
        .operation
        .clone()
        .filter(|operation| operation.operation_id == request.context)
    {
        finish_configuration(state, saved, operation, result, changes)?;
        return Ok(());
    }
    Err(errors::invalid("Unexpected ACP response context"))
}

#[cfg(test)]
mod tests {
    use super::{clean_agent_error, observed_model_refusal};

    #[test]
    fn provider_subscription_refusal_keeps_the_actionable_message() {
        let raw = "Internal error: Your coding plan package has expired and is temporarily unavailable. Renew the subscription on the provider website.";
        let clean = clean_agent_error(raw);

        assert_eq!(
            clean,
            "Your coding plan package has expired and is temporarily unavailable. Renew the subscription on the provider website."
        );
        assert!(observed_model_refusal(-32603, &clean));
    }

    #[test]
    fn generic_internal_errors_do_not_mark_a_model_as_refused() {
        let clean = clean_agent_error("Internal error: Temporary upstream failure");

        assert_eq!(clean, "Temporary upstream failure");
        assert!(!observed_model_refusal(-32603, &clean));
        assert!(!observed_model_refusal(
            -32000,
            "Model is disabled for this account"
        ));
    }
}
