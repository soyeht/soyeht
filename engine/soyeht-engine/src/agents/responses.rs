use super::{
    Changes,
    harness_sign_in::{self, SignInChange},
    native_error, prompt_errors,
    purpose::{ProbeStep, Purpose},
    sign_in::needs_login,
    state::{Phase, State},
    transcript,
};
use crate::model::{config_failed, pending, turn_ended};
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
use soyeht_protocol::{PaneConfigChangedDraft, PaneSessionStartedDraft};
use soyeht_protocol::{
    PaneState, ProbeCapability, ProbeResult, TranscriptKind, TranscriptRole, TurnStopReason, event,
};
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
    purpose: &Purpose,
    now_ms: i64,
) -> Result<()> {
    if method == "session/prompt" {
        state.active_output_observed = false;
        state.active_silent_since_ms = Some(now_ms);
    }
    changes.outgoing.push(
        state
            .client
            .request(method, params, &purpose.context(), now_ms as u64)
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
            &Purpose::Load,
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
            &Purpose::Open,
            runtime.now_ms,
        )
    }
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
            changes.events.push(pending(&PaneConfigChangedDraft {
                pane_id: saved.pane.pane_id.clone(),
                config_id: operation.config_id.clone(),
                value: operation.value.clone(),
                options: state.options.values.clone(),
                session_preserved: true,
            })?);
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
            changes.events.push(config_failed(
                &saved.pane.pane_id,
                operation.config_id.clone(),
                operation.value.clone(),
                &error.to_string(),
            )?);
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

fn auth_response(
    state: &mut State,
    saved: &StoredPane,
    changes: &mut Changes,
    result: NativeResult,
) -> Result<()> {
    let ok = result.is_ok();
    state.auth_required = !ok;
    if !ok {
        changes.sign_in = Some(SignInChange::forget(&state.launch.id));
    }
    state.phase = if ok {
        Phase::RestartAfterProbe
    } else {
        Phase::NeedsLogin
    };
    state.probe_started_ms = None;
    changes.clear_failed_probes = ok;
    let reason = result
        .err()
        .map(|error| error.message)
        .unwrap_or_else(|| "Sign-in completed".into());
    changes.events.push(super::authentication::auth_finished(
        &saved.pane.pane_id,
        ok,
        &reason,
    )?);
    Ok(())
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
    // A process that has not authenticated yet cannot open the session the person's
    // sign-in was for, so it is signed in with the method the person chose for this harness
    // before the probe or the session.
    let method = harness_sign_in::method_to_send(store, &state.launch.id, &info)?;
    state.initialized = Some(info);
    if let Some(method) = method {
        return call(
            state,
            changes,
            "authenticate",
            json!({"methodId":method}),
            &Purpose::Reauthenticate,
            runtime.now_ms,
        );
    }
    after_initialize(state, changes, saved, (store, runtime))
}

fn reauthentication_response(
    store: &(impl StorePort + TaskStorePort),
    runtime: &Runtime<'_>,
    state: &mut State,
    saved: &mut StoredPane,
    changes: &mut Changes,
    result: NativeResult,
) -> Result<()> {
    if result.is_ok() {
        return after_initialize(state, changes, saved, (store, runtime));
    }
    refuse_reauthentication(state, saved, changes)
}

/// The agent no longer accepts that sign-in: the person signs in again.
fn refuse_reauthentication(
    state: &mut State,
    saved: &mut StoredPane,
    changes: &mut Changes,
) -> Result<()> {
    changes.sign_in = Some(SignInChange::forget(&state.launch.id));
    needs_login(state, saved, changes)
}

/// The probe of an agent version nobody has probed yet, or else the pane's session.
fn after_initialize(
    state: &mut State,
    changes: &mut Changes,
    saved: &StoredPane,
    (store, runtime): (&(impl StorePort + TaskStorePort), &Runtime<'_>),
) -> Result<()> {
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
        &Purpose::Probe(ProbeStep::New),
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
                &Purpose::Probe(ProbeStep::List),
                now_ms,
            )
        }
        Err(error) if soyeht_acp::rpc::is_auth_required(&error) => {
            needs_login(state, saved, changes)
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
    let Purpose::Probe(step) = Purpose::parse(&request.context) else {
        return Err(errors::internal("Unknown probe response"));
    };
    let mut classified = session::probe_result(result.as_ref());
    if step == ProbeStep::Config
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
    match step {
        ProbeStep::List => {
            state.phase = Phase::ProbeLoad;
            call(
                state,
                changes,
                "session/load",
                client::load_session(&saved.pane.cwd, &id),
                &Purpose::Probe(ProbeStep::Load),
                now_ms,
            )
        }
        ProbeStep::Load => probe_configuration(state, changes, &id, now_ms, timestamp),
        ProbeStep::Config => {
            state.phase = Phase::RestartAfterProbe;
            Ok(())
        }
        ProbeStep::New => Err(errors::internal("Unknown probe response")),
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
        call(
            state,
            changes,
            method,
            params,
            &Purpose::Probe(ProbeStep::Config),
            now_ms,
        )
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
    purpose: &Purpose,
    changes: &mut Changes,
    result: NativeResult,
) -> Result<()> {
    match result {
        Ok(value) => session_opened(store, state, saved, purpose, changes, &value),
        Err(error) if soyeht_acp::rpc::is_auth_required(&error) => {
            needs_login(state, saved, changes)
        }
        Err(error) if *purpose == Purpose::Load && matches!(error.code, -32601 | -32002) => {
            session_lost(store, runtime, state, saved, changes, error.message)
        }
        Err(error) => Err(errors::harness_unavailable(error.message)),
    }
}

fn session_opened(
    store: &impl StorePort,
    state: &mut State,
    saved: &mut StoredPane,
    purpose: &Purpose,
    changes: &mut Changes,
    value: &Value,
) -> Result<()> {
    if *purpose == Purpose::Open {
        let id = session::session_id(value).map_err(native_error)?;
        if id.chars().count() > 128 {
            return Err(errors::invalid(
                "Native session ID exceeds the public protocol bound",
            ));
        }
        state.session_id = Some(id);
    }
    if *purpose == Purpose::Load {
        // The replay replaces the stored transcript, so a prompt written while
        // it waited is gone; it is written again when it starts.
        state.transcribed_prompts.clear();
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
    if value.get("configOptions").is_some()
        || value.get("modes").is_some()
        || *purpose == Purpose::Open
    {
        super::configuration::open_session(
            state,
            Options::from_response(value).map_err(native_error)?,
        )?;
    }
    state.phase = Phase::Ready;
    // A pane with an open session waits for a prompt; it is not still starting.
    saved.pane.state = PaneState::Idle;
    saved.pane.phrase = "Agent session ready".into();
    changes.events.push(pending(&PaneSessionStartedDraft {
        pane_id: saved.pane.pane_id.clone(),
        harness: state.launch.id.clone(),
        version: initialized_version(state)?.into(),
        session_id: state
            .session_id
            .clone()
            .ok_or_else(|| errors::internal("Started session has no id"))?,
        generation: saved.pane.generation,
        options: state.options.values.clone(),
    })?);
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
        event::PANE_SESSION_LOST.into(),
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
        Ok(value) => {
            let reason: TurnStopReason = serde_json::from_value(Value::String(
                session::stop_reason(&value)
                    .map_err(native_error)?
                    .to_owned(),
            ))
            .map_err(|_| errors::internal("Unknown stop reason"))?;
            match prompt_errors::rejected_model_reply(store, &saved.pane.pane_id, state)? {
                Some(reply) if reason == TurnStopReason::EndTurn => {
                    let failure = prompt_errors::Failure::RejectedModelReply;
                    prompt_errors::report(
                        store, saved, state, changes, &reply, failure, timestamp,
                    )?;
                    TurnStopReason::Error
                }
                _ => reason,
            }
        }
        Err(error) => {
            let clean = prompt_errors::clean_agent_error(&error.message);
            let failure = prompt_errors::Failure::Rpc(error.code);
            prompt_errors::report(store, saved, state, changes, &clean, failure, timestamp)?;
            TurnStopReason::Error
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
    if cancel_requested && reason == TurnStopReason::Cancelled {
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
    if !matches!(reason, TurnStopReason::Error | TurnStopReason::Cancelled) {
        state.auth_required = false;
    }
    saved.pane.state = PaneState::Idle;
    saved.pane.phrase = "Agent turn ended".into();
    changes
        .events
        .push(turn_ended(&saved.pane.pane_id, context.into(), reason)?);
    if authentication_required {
        needs_login(state, saved, changes)?;
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
    match Purpose::parse(&request.context) {
        Purpose::Authenticate => {
            return auth_response(state, saved, changes, result);
        }
        Purpose::Reauthenticate => {
            return reauthentication_response(store, runtime, state, saved, changes, result);
        }
        Purpose::Logout => {
            return super::account::logout_response(
                state,
                saved,
                changes,
                result.map(|_| ()).map_err(|error| error.message),
            );
        }
        Purpose::Initialize => {
            return initialize_response(store, runtime, state, saved, changes, result);
        }
        Purpose::Probe(ProbeStep::New) => {
            return probe_new_response(state, saved, changes, result, runtime.now_ms);
        }
        Purpose::Probe(_) => {
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
        purpose @ (Purpose::Open | Purpose::Load) => {
            return session_response(store, runtime, state, saved, &purpose, changes, result);
        }
        Purpose::Identified(_) => {}
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
mod tests;
