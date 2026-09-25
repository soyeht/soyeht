use super::{
    Changes, native_error,
    state::{Phase, State},
    transcript,
};
use crate::{
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_acp::session::{self, Update};
use soyeht_protocol::{ConfigValue, TokenUsage, TranscriptKind, TranscriptRole};

fn append_replayed_user(
    store: &impl StorePort,
    pane: &str,
    state: &mut State,
    generation: u32,
    text: &str,
    message_id: Option<&str>,
    changes: &mut Changes,
) -> Result<()> {
    transcript::append_text_role(
        store,
        pane,
        state,
        generation,
        "user",
        transcript::TextChunk { text, message_id },
        changes,
    )
}

fn update_user(
    store: &impl StorePort,
    pane: &str,
    generation: u32,
    state: &mut State,
    content: Value,
    message_id: Option<&str>,
    changes: &mut Changes,
) -> Result<()> {
    // Ordinary turns already have the accepted user prompt in the journal.
    // Only native load reconstructs it from replayed chunks.
    if state.phase != Phase::Loading {
        return Ok(());
    }
    if content["type"] != "text" {
        return Err(errors::invalid("Unsupported replay content"));
    }
    let text = content["text"]
        .as_str()
        .ok_or_else(|| errors::invalid("Replay has no text"))?;
    append_replayed_user(store, pane, state, generation, text, message_id, changes)
}

fn update_tool(
    store: &impl StorePort,
    pane: &str,
    generation: u32,
    state: &mut State,
    update: bool,
    fields: &Value,
    changes: &mut Changes,
) -> Result<()> {
    let id = errors::param_str(fields, "toolCallId")?;
    if id.is_empty() || id.chars().count() > 128 {
        return Err(errors::invalid("Tool ID exceeds public protocol bounds"));
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
    let mut tool = item.tool.as_ref().map(|tool| json!(tool)).unwrap_or_else(
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
    item.tool =
        Some(serde_json::from_value(tool).map_err(|_| errors::invalid("Invalid tool update"))?);
    state
        .open_messages
        .insert(key, transcript::sequence(&item.seq)?);
    transcript::put(
        changes,
        pane,
        item,
        if update {
            "tool_call_update"
        } else {
            "tool_call"
        },
    )
}

fn update_plan(
    pane: &str,
    generation: u32,
    state: &mut State,
    entries: Value,
    changes: &mut Changes,
) -> Result<()> {
    let mut item = transcript::new_message(
        state,
        generation,
        TranscriptRole::Assistant,
        TranscriptKind::Plan,
        String::new(),
        true,
    )?;
    item.plan =
        Some(serde_json::from_value(entries).map_err(|_| errors::invalid("Invalid plan entries"))?);
    transcript::put(changes, pane, item, "plan")
}

fn update_usage(
    pane: &str,
    generation: u32,
    state: &mut State,
    value: &Value,
    changes: &mut Changes,
) -> Result<()> {
    let used = value["used"]
        .as_u64()
        .ok_or_else(|| errors::invalid("Invalid token usage"))?;
    let size = value["size"]
        .as_u64()
        .ok_or_else(|| errors::invalid("Invalid context size"))?;
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
    transcript::put(changes, pane, item, "usage")
}

fn apply_mode(state: &mut State, mode: &str) -> Result<()> {
    super::configuration::require_native_policy(mode)?;
    if state.pending_policy.is_none() && state.requested_options.is_empty() {
        super::configuration::observe_mode(state, mode, false)
    } else {
        state.effective_mode = Some(mode.into());
        Ok(())
    }
}

fn configuration_event(
    pane: &str,
    generation: u32,
    state: &mut State,
    text: &str,
    changes: &mut Changes,
) -> Result<()> {
    let item = transcript::new_message(
        state,
        generation,
        TranscriptRole::System,
        TranscriptKind::System,
        text.into(),
        true,
    )?;
    changes.messages.push(item.clone());
    if !changes.replay_messages {
        changes.events.push(("pane.update".into(),json!({"paneId":pane,"kind":"config_option_update","generation":generation,"seq":item.seq,"localId":item.local_id,"options":state.options.values})));
    }
    Ok(())
}

fn update_configuration(
    pane: &str,
    generation: u32,
    state: &mut State,
    options: soyeht_acp::config::Options,
    changes: &mut Changes,
) -> Result<()> {
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
        apply_mode(state, &mode)?;
    }
    configuration_event(
        pane,
        generation,
        state,
        "Agent updated its configuration",
        changes,
    )
}

fn update_mode(
    pane: &str,
    generation: u32,
    state: &mut State,
    mode: String,
    changes: &mut Changes,
) -> Result<()> {
    super::configuration::require_native_policy(&mode)?;
    if let Some(option) = state
        .options
        .values
        .iter_mut()
        .find(|option| option.category.as_deref() == Some("mode") || option.id == "mode")
    {
        option.current_value = ConfigValue::V0(mode.clone());
    }
    apply_mode(state, &mode)?;
    configuration_event(pane, generation, state, "Agent updated its mode", changes)
}

pub fn update(
    store: &impl StorePort,
    pane: &str,
    generation: u32,
    state: &mut State,
    params: &Value,
    changes: &mut Changes,
) -> Result<bool> {
    let expected = if matches!(
        state.phase,
        Phase::ProbeLoad | Phase::ProbeList | Phase::ProbeConfig
    ) {
        state.probe_session_id.as_deref()
    } else {
        state.session_id.as_deref()
    };
    let Some(expected) = expected else {
        return Err(errors::invalid("Update arrived before a session identity"));
    };
    let update = session::update(expected, params).map_err(native_error)?;
    if matches!(
        state.phase,
        Phase::ProbeLoad | Phase::ProbeList | Phase::ProbeConfig
    ) {
        return Ok(false);
    }
    let Some(update) = update else {
        return Ok(false);
    };
    if state.phase == Phase::Loading {
        changes.replay_messages = true;
        let role = match &update {
            Update::Message { thought: true, .. } => "thought",
            Update::Message { .. } => "message",
            Update::UserMessage { .. } => "user",
            _ => "other",
        };
        if state.replay_role.as_deref() != Some(role) {
            transcript::complete(store, pane, state, changes)?;
            state.replay_role = Some(role.into());
        }
    }
    let observed_turn_output = match update {
        Update::UserMessage {
            content,
            message_id,
        } => {
            update_user(
                store,
                pane,
                generation,
                state,
                content,
                message_id.as_deref(),
                changes,
            )?;
            false
        }
        Update::Message {
            thought,
            content,
            message_id,
        } => {
            if content["type"] != "text" {
                return Err(errors::invalid("This pane accepts text response blocks"));
            }
            let text = content["text"]
                .as_str()
                .ok_or_else(|| errors::invalid("Text response has no text"))?;
            transcript::append_text(
                store,
                pane,
                state,
                generation,
                thought,
                transcript::TextChunk {
                    text,
                    message_id: message_id.as_deref(),
                },
                changes,
            )?;
            true
        }
        Update::Tool { update, fields } => {
            update_tool(store, pane, generation, state, update, &fields, changes)?;
            true
        }
        Update::Plan(entries) => {
            update_plan(pane, generation, state, entries, changes)?;
            true
        }
        // Some adapters emit accounting while a provider is wedged. Usage is
        // useful transcript data, but it is not evidence that the requested
        // turn has produced user-visible progress.
        Update::Usage(value) => {
            update_usage(pane, generation, state, &value, changes)?;
            false
        }
        Update::Configuration(options) => {
            update_configuration(pane, generation, state, options, changes)?;
            false
        }
        Update::Mode(mode) => {
            update_mode(pane, generation, state, mode, changes)?;
            false
        }
    };
    Ok(observed_turn_output)
}
