use super::{Changes, native_error, pending::UserOperation, responses, state::State, transcript};
use crate::{
    core::Core,
    errors::{self, Result},
};
use serde_json::json;
use soyeht_acp::config::{FULL_ACCESS_KIND, MODE_CATEGORY, Options, Route};
use soyeht_protocol::{
    ConfigChoice, ConfigOperation, ConfigOption, ConfigType, ConfigValue, TranscriptKind,
    TranscriptRole,
};
use uuid::Uuid;

/// `State::requested_options` key of the permission mode.
pub const MODE_KEY: &str = "mode";

/// Permission modes the engine enforces itself, only for agents that
/// announce no native modes. Agents with native modes answer their own
/// permission policy; the engine then never approves on the person's behalf.
pub(super) const ASK: &str = "ask";
pub(super) const ACCEPT_EDITS: &str = "accept_edits";
pub(super) const ALLOW_ALL: &str = "allow_all";

fn client_modes(current: Option<&str>) -> ConfigOption {
    let choice = |value: &str, name: &str, description: &str, kind: &str| ConfigChoice {
        value: value.into(),
        name: name.into(),
        description: Some(description.into()),
        kind: Some(kind.into()),
    };
    let options = vec![
        choice(ASK, "Ask every time", "Ask before every action", "standard"),
        choice(
            ACCEPT_EDITS,
            "Accept edits",
            "Approve file edits inside the pane directory",
            "standard",
        ),
        choice(
            ALLOW_ALL,
            "Allow all",
            "Approve every action",
            FULL_ACCESS_KIND,
        ),
    ];
    let current = current
        .filter(|value| options.iter().any(|choice| choice.value == *value))
        .unwrap_or(ASK);
    ConfigOption {
        id: MODE_CATEGORY.into(),
        name: "Mode".into(),
        description: Some("Enforced by Soyeht: this agent announces no permission modes".into()),
        category: Some(MODE_CATEGORY.into()),
        r#type: ConfigType::Select,
        current_value: ConfigValue::V0(current.into()),
        options,
    }
}

/// The mode the engine enforces, when the agent announces none of its own.
pub(super) fn engine_policy(state: &State) -> Option<&str> {
    let option = state.options.mode()?;
    (state.options.route(&option.id) == Some(&Route::Client))
        .then(|| state.options.current_mode())
        .flatten()
}

/// The announced option a `requested_options` key selects.
pub(super) fn requested_option<'a>(options: &'a Options, key: &str) -> Option<&'a ConfigOption> {
    let category = if key == "effort" {
        "thought_level"
    } else {
        key
    };
    options
        .values
        .iter()
        .find(|option| option.category.as_deref() == Some(category) || option.id == category)
}

/// The `requested_options` key an announced option satisfies.
fn requested_key(option: &ConfigOption) -> &str {
    match option.category.as_deref().unwrap_or(&option.id) {
        "thought_level" => "effort",
        other => other,
    }
}

/// Gives an agent that announces no modes the engine's own, starting in the
/// pane's mode when it is one of them.
fn ensure_engine_modes(state: &mut State) -> Result<()> {
    let mode = state.mode.clone();
    state
        .options
        .ensure_mode(|| client_modes(mode.as_deref()))
        .map_err(native_error)
}

/// Adopts the options a new or reloaded session announced. Agents without
/// native modes get the engine's modes; the pane's mode is queued for
/// re-application when the agent offers it but reports another one.
pub(super) fn open_session(state: &mut State, options: Options) -> Result<()> {
    state.options = options;
    // A failure belongs to the session that refused; a new one tries again.
    state.initial_config_failed = false;
    ensure_engine_modes(state)?;
    match state.mode.clone() {
        Some(mode)
            if engine_policy(state).is_none()
                && state.options.offers_mode(&mode)
                && state.options.current_mode() != Some(mode.as_str()) =>
        {
            state
                .requested_options
                .insert(MODE_KEY.into(), ConfigValue::V0(mode));
        }
        _ => keep_harness_mode(state),
    }
    Ok(())
}

/// Stops applying the pane's mode: the mode the harness reports becomes the
/// pane's mode, so a mode the harness does not offer is never restored later.
fn keep_harness_mode(state: &mut State) {
    state.requested_options.remove(MODE_KEY);
    state.mode = state.options.current_mode().map(str::to_owned);
}

/// The harness refused to enter the pane's mode. The queue continues in the
/// mode it reports, and the transcript says so.
pub(super) fn refused_mode(
    state: &mut State,
    generation: u32,
    refused: &ConfigValue,
    changes: &mut Changes,
) -> Result<()> {
    keep_harness_mode(state);
    let name = |value: &str| {
        state
            .options
            .mode()
            .and_then(|option| option.options.iter().find(|choice| choice.value == value))
            .map_or_else(|| value.to_owned(), |choice| choice.name.clone())
    };
    let refused = match refused {
        ConfigValue::V0(value) => name(value),
        ConfigValue::V1(value) => value.to_string(),
    };
    let kept = state
        .mode
        .as_deref()
        .map_or_else(|| "its own mode".into(), name);
    let notice = transcript::new_message(
        state,
        generation,
        TranscriptRole::System,
        TranscriptKind::System,
        format!("The agent did not enter {refused}; it stays in {kept}"),
        true,
    )?;
    changes.messages.push(notice);
    Ok(())
}

/// Records a confirmed option; a confirmed mode becomes the pane's mode.
pub(super) fn confirmed(state: &mut State, options: Options, config: &str) {
    state.options = options;
    if let Some(option) = state
        .options
        .values
        .iter()
        .find(|option| option.id == config)
    {
        let key = requested_key(option).to_owned();
        state.requested_options.remove(&key);
        if key == MODE_KEY {
            state.mode = state.options.current_mode().map(str::to_owned);
        }
    }
}

/// Applies an announcement the agent made on its own. A mode it chose itself
/// becomes the pane's mode unless the engine is still applying one.
pub(super) fn agent_changed(state: &mut State, options: Options) -> Result<()> {
    state.options = state.options.adopt(options).map_err(native_error)?;
    ensure_engine_modes(state)?;
    if state.phase == super::state::Phase::Ready
        && !state.requested_options.contains_key(MODE_KEY)
        && state.operation.is_none()
    {
        state.mode = state.options.current_mode().map(str::to_owned);
    }
    Ok(())
}

pub struct ChangeRequest<'a> {
    pub config: &'a str,
    pub value: ConfigValue,
    pub user: Option<UserOperation>,
    pub started_at: String,
    pub now_ms: i64,
}

/// Starts a configuration change. Returns the pending operation, or `None`
/// when the engine enforces the option itself and applied it at once.
pub fn begin(
    state: &mut State,
    pane: &str,
    request: ChangeRequest<'_>,
    changes: &mut Changes,
) -> Result<Option<String>> {
    let ChangeRequest {
        config,
        value,
        user,
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
    let Some((method, params)) = state
        .options
        .request(session, config, &value)
        .map_err(native_error)?
    else {
        let options = state
            .options
            .confirmed(config, &value, &json!({}))
            .map_err(native_error)?;
        confirmed(state, options, config);
        changes.events.push(("pane.config_changed".into(),json!({"paneId":pane,"configId":config,"value":value,"options":state.options.values,"sessionPreserved":true})));
        return Ok(None);
    };
    let id = Uuid::new_v4().to_string();
    state.operation = Some(ConfigOperation {
        operation_id: id.clone(),
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
    Ok(Some(id))
}

pub fn applied(value: &ConfigValue) -> serde_json::Value {
    json!({"applied":true,"effective":value,"sessionPreserved":true})
}

impl Core {
    /// Applies the options a pane was opened with, one native change at a
    /// time. A mode the agent does not offer is dropped: the agent keeps its
    /// own mode rather than a guessed one.
    pub fn configure_initial_agent(&mut self, pane: &str) -> Result<()> {
        let mut state = self.agent_state(pane)?;
        if state.operation.is_some() || state.initial_config_failed {
            return Ok(());
        }
        let mut changes = Changes::default();
        while let Some((key, desired)) = state
            .requested_options
            .first_key_value()
            .map(|(k, v)| (k.clone(), v.clone()))
        {
            let offered = requested_option(&state.options, &key).filter(|_| {
                key != MODE_KEY
                    || matches!(&desired, ConfigValue::V0(mode) if state.options.offers_mode(mode))
            });
            let Some(option) = offered.cloned() else {
                if key == MODE_KEY {
                    keep_harness_mode(&mut state);
                    continue;
                }
                state.initial_config_failed = true;
                changes.events.push(("pane.config_failed".into(),json!({"paneId":pane,"configId":key,"value":desired,"reason":"Requested selector is not offered by this session"})));
                break;
            };
            if option.current_value == desired {
                let options = state.options.clone();
                confirmed(&mut state, options, &option.id);
                continue;
            }
            match begin(
                &mut state,
                pane,
                ChangeRequest {
                    config: &option.id,
                    value: desired.clone(),
                    user: None,
                    started_at: self.timestamp(),
                    now_ms: self.now_ms(),
                },
                &mut changes,
            ) {
                Ok(Some(_)) => {
                    state.initial_option = Some(key);
                    break;
                }
                Ok(None) => {}
                Err(error) => {
                    changes.events.push(("pane.config_failed".into(),json!({"paneId":pane,"configId":option.id,"value":desired,"reason":error.to_string()})));
                    if key == MODE_KEY {
                        let generation = self.panes[pane].stored.pane.generation;
                        refused_mode(&mut state, generation, &desired, &mut changes)?;
                        continue;
                    }
                    state.initial_config_failed = true;
                    break;
                }
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
}
