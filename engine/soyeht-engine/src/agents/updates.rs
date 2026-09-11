use super::{
    Changes, native_error,
    state::{Phase, State},
    transcript,
};
use crate::errors::{self, Result};
use serde_json::{Value, json};
use soyeht_acp::session::{self, Update};
use soyeht_protocol::{ConfigValue, ErrorKind, TokenUsage, TranscriptKind, TranscriptRole};
use soyeht_store::database::Store;

fn append_replayed_user(
    store: &Store,
    pane: &str,
    state: &mut State,
    generation: u32,
    text: &str,
    changes: &mut Changes,
) -> Result<()> {
    transcript::append_text_role(store, pane, state, generation, "user", text, changes)
}

pub fn update(
    store: &Store,
    pane: &str,
    generation: u32,
    state: &mut State,
    params: &Value,
    changes: &mut Changes,
) -> Result<()> {
    let expected = if matches!(
        state.phase,
        Phase::ProbeLoad | Phase::ProbeList | Phase::ProbeConfig
    ) {
        state.probe_session_id.as_deref()
    } else {
        state.session_id.as_deref()
    };
    let Some(expected) = expected else {
        return Err(errors::error(
            ErrorKind::InvalidParams,
            "Update arrived before a session identity",
        ));
    };
    let update = session::update(expected, params).map_err(native_error)?;
    if matches!(
        state.phase,
        Phase::ProbeLoad | Phase::ProbeList | Phase::ProbeConfig
    ) {
        return Ok(());
    }
    let Some(update) = update else { return Ok(()) };
    if state.phase == Phase::Loading {
        changes.replay_messages = true;
        let role = match &update {
            Update::Message { thought: true, .. } => "thought",
            Update::Message { .. } => "message",
            Update::UserMessage(_) => "user",
            _ => "other",
        };
        if state.replay_role.as_deref() != Some(role) {
            transcript::complete(store, pane, state, changes)?;
            state.replay_role = Some(role.into());
        }
    }
    match update {
        Update::UserMessage(content) => {
            // Ordinary turns already have the accepted user prompt in the
            // journal. Only native load reconstructs it from replayed chunks.
            if state.phase == Phase::Loading {
                if content["type"] != "text" {
                    return Err(errors::error(
                        ErrorKind::InvalidParams,
                        "Unsupported replay content",
                    ));
                }
                let text = content["text"]
                    .as_str()
                    .ok_or_else(|| errors::error(ErrorKind::InvalidParams, "Replay has no text"))?;
                append_replayed_user(store, pane, state, generation, text, changes)?;
            }
        }
        Update::Message { thought, content } => {
            if content["type"] != "text" {
                return Err(errors::error(
                    ErrorKind::InvalidParams,
                    "This pane accepts text response blocks",
                ));
            }
            let text = content["text"].as_str().ok_or_else(|| {
                errors::error(ErrorKind::InvalidParams, "Text response has no text")
            })?;
            transcript::append_text(store, pane, state, generation, thought, text, changes)?;
        }
        Update::Tool { update, fields } => {
            let id = fields["toolCallId"].as_str().expect("checked tool ID");
            if id.is_empty() || id.chars().count() > 128 {
                return Err(errors::error(
                    ErrorKind::InvalidParams,
                    "Tool ID exceeds public protocol bounds",
                ));
            }
            let key = format!("tool:{id}");
            let prior = state
                .open_messages
                .get(&key)
                .map(|seq| transcript::message(store, pane, *seq))
                .transpose()?
                .flatten();
            let mut item = match prior {
                Some(item) => item,
                None => transcript::new_message(
                    state,
                    generation,
                    TranscriptRole::Assistant,
                    TranscriptKind::Tool,
                    String::new(),
                    false,
                )?,
            };
            let mut tool = item.tool.as_ref().map(|t| json!(t)).unwrap_or_else(
                || json!({"toolCallId":id,"title":"Tool","status":"pending","locations":[]}),
            );
            for name in [
                "title",
                "kind",
                "status",
                "content",
                "locations",
                "rawInput",
                "rawOutput",
            ] {
                if let Some(value) = fields.get(name) {
                    tool[name] = value.clone();
                }
            }
            item.complete = matches!(tool["status"].as_str(), Some("completed" | "failed"));
            item.tool = Some(
                serde_json::from_value(tool)
                    .map_err(|_| errors::error(ErrorKind::InvalidParams, "Invalid tool update"))?,
            );
            state
                .open_messages
                .insert(key, item.seq.parse().expect("allocated sequence"));
            transcript::put(
                changes,
                pane,
                item,
                if update {
                    "tool_call_update"
                } else {
                    "tool_call"
                },
            )?;
        }
        Update::Plan(entries) => {
            let mut item = transcript::new_message(
                state,
                generation,
                TranscriptRole::Assistant,
                TranscriptKind::Plan,
                String::new(),
                true,
            )?;
            item.plan =
                Some(serde_json::from_value(entries).map_err(|_| {
                    errors::error(ErrorKind::InvalidParams, "Invalid plan entries")
                })?);
            transcript::put(changes, pane, item, "plan")?;
        }
        Update::Usage(value) => {
            let used = value["used"]
                .as_u64()
                .ok_or_else(|| errors::error(ErrorKind::InvalidParams, "Invalid token usage"))?;
            let size = value["size"]
                .as_u64()
                .ok_or_else(|| errors::error(ErrorKind::InvalidParams, "Invalid context size"))?;
            let mut item = transcript::new_message(
                state,
                generation,
                TranscriptRole::Assistant,
                TranscriptKind::Usage,
                String::new(),
                true,
            )?;
            item.usage = Some(TokenUsage {
                input_tokens: None,
                output_tokens: None,
                total_tokens: used.to_string(),
                context_size: Some(size.to_string()),
            });
            transcript::put(changes, pane, item, "usage")?;
        }
        Update::Configuration(options) => {
            state.options = options;
            let mode = state
                .options
                .values
                .iter()
                .find(|option| option.category.as_deref() == Some("mode") || option.id == "mode")
                .and_then(|option| match &option.current_value {
                    ConfigValue::V0(value) => Some(value.clone()),
                    _ => None,
                });
            if let Some(mode) = mode {
                super::configuration::require_native_policy(&mode)?;
                if state.pending_policy.is_none() && state.requested_options.is_empty() {
                    super::configuration::observe_mode(state, &mode, false)?;
                } else {
                    state.effective_mode = Some(mode);
                }
            }
            let item = transcript::new_message(
                state,
                generation,
                TranscriptRole::System,
                TranscriptKind::System,
                "Agent updated its configuration".into(),
                true,
            )?;
            changes.messages.push(item.clone());
            if !changes.replay_messages {
                changes.events.push(("pane.update".into(),json!({"paneId":pane,"kind":"config_option_update","generation":generation,"seq":item.seq,"localId":item.local_id,"options":state.options.values})));
            }
        }
        Update::Mode(mode) => {
            super::configuration::require_native_policy(&mode)?;
            if let Some(option) = state
                .options
                .values
                .iter_mut()
                .find(|o| o.category.as_deref() == Some("mode") || o.id == "mode")
            {
                option.current_value = ConfigValue::V0(mode.clone());
            }
            if state.pending_policy.is_none() && state.requested_options.is_empty() {
                super::configuration::observe_mode(state, &mode, false)?;
            } else {
                state.effective_mode = Some(mode);
            }
            let item = transcript::new_message(
                state,
                generation,
                TranscriptRole::System,
                TranscriptKind::System,
                "Agent updated its mode".into(),
                true,
            )?;
            changes.messages.push(item.clone());
            if !changes.replay_messages {
                changes.events.push(("pane.update".into(),json!({"paneId":pane,"kind":"config_option_update","generation":generation,"seq":item.seq,"localId":item.local_id,"options":state.options.values})));
            }
        }
    }
    Ok(())
}
