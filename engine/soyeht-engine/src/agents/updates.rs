use super::{
    Changes, native_error,
    state::{InternalContinuation, Phase, ReplayedUserMessage, State},
    subagents::{self, Scope},
    transcript,
};
use crate::{
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_acp::{
    session::{self, Update},
    subagents::Lifecycle,
};
use soyeht_protocol::{ConfigValue, TokenUsage, TranscriptKind, TranscriptMessage, TranscriptRole};

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
        transcript::TextChunk {
            text,
            message_id,
            subagent: None,
        },
        changes,
    )
    .map(drop)
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
    let Some(continuation) = state
        .internal_continuation
        .clone()
        .filter(InternalContinuation::has_prompt)
    else {
        return append_replayed_user(store, pane, state, generation, text, message_id, changes);
    };
    // Adapters replay a long prompt in several chunks under one messageId.
    // Hold each user message until it is complete so the continuation prompt
    // is recognized as a whole and none of it reaches the transcript.
    if state
        .replay_user
        .as_ref()
        .is_some_and(|pending| pending.message_id.as_deref() != message_id)
    {
        finish_replayed_user(store, pane, generation, state, changes)?;
        // This new user message starts the next turn, which is visible.
        state.replay_continuation_turn = false;
    }
    let pending = state
        .replay_user
        .get_or_insert_with(|| ReplayedUserMessage {
            message_id: message_id.map(Into::into),
            text: String::new(),
            streaming: false,
        });
    let text = if pending.streaming {
        text.to_owned()
    } else {
        pending.text.push_str(text);
        if continuation.could_match(pending.text.len()) {
            return Ok(());
        }
        // Too long to be the continuation prompt: stream it as a user turn.
        pending.streaming = true;
        std::mem::take(&mut pending.text)
    };
    append_replayed_user(store, pane, state, generation, &text, message_id, changes)
}

/// Emits the held replayed user message unless it is the engine's own
/// continuation prompt, which the leading handover marker already represents.
/// The agent's reply to that prompt is hidden too, as it is during the switch.
pub(super) fn finish_replayed_user(
    store: &impl StorePort,
    pane: &str,
    generation: u32,
    state: &mut State,
    changes: &mut Changes,
) -> Result<()> {
    let Some(pending) = state.replay_user.take() else {
        return Ok(());
    };
    if pending.streaming {
        return Ok(());
    }
    if state
        .internal_continuation
        .as_ref()
        .is_some_and(|continuation| continuation.matches_replayed_prompt(&pending.text))
    {
        state.replay_continuation_turn = true;
        return Ok(());
    }
    append_replayed_user(
        store,
        pane,
        state,
        generation,
        &pending.text,
        pending.message_id.as_deref(),
        changes,
    )
}

/// The transcript item holding the latest reported state of a tool call in
/// the current prompt, if the agent has reported it.
pub fn tracked_tool(
    store: &impl StorePort,
    pane: &str,
    state: &State,
    id: &str,
) -> Result<Option<TranscriptMessage>> {
    state
        .open_messages
        .get(&format!("tool:{id}"))
        .map(|seq| transcript::message(store, pane, *seq))
        .transpose()
        .map(Option::flatten)
}

struct ToolUpdate<'a> {
    update: bool,
    fields: &'a Value,
    subagent: Option<&'a str>,
}

fn update_tool(
    store: &impl StorePort,
    pane: &str,
    generation: u32,
    state: &mut State,
    tool_update: ToolUpdate<'_>,
    changes: &mut Changes,
) -> Result<()> {
    let ToolUpdate {
        update,
        fields,
        subagent,
    } = tool_update;
    let id = errors::param_str(fields, "toolCallId")?;
    if id.is_empty() || id.chars().count() > 128 {
        return Err(errors::invalid("Tool ID exceeds public protocol bounds"));
    }
    // Claude Code reports a subagent's model on the Agent call that spawned
    // it; that call is the subagent's row, not a tool row of its own.
    if subagent.is_none()
        && let Some(report) = soyeht_acp::subagents::report(fields)
        && super::subagents::reported(store, pane, state, report, changes)?
    {
        return Ok(());
    }
    let key = transcript::open_key("tool", subagent, Some(id));
    let prior = match subagent {
        None => tracked_tool(store, pane, state, id)?,
        Some(_) => state
            .open_messages
            .get(&key)
            .map(|seq| transcript::message(store, pane, *seq))
            .transpose()?
            .flatten(),
    };
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
    item.subagent_id = subagent.map(Into::into);
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
    subagent: Option<&str>,
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
    item.subagent_id = subagent.map(Into::into);
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
    super::configuration::agent_changed(state, options)?;
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
    let mut options = state.options.clone();
    if super::configuration::engine_policy(state).is_none()
        && let Some(option) = options.mode_mut()
    {
        option.current_value = ConfigValue::V0(mode);
    }
    super::configuration::agent_changed(state, options)?;
    configuration_event(pane, generation, state, "Agent updated its mode", changes)
}

/// Where a session update lands and when it happened, if it is live.
struct Context<'a> {
    pane: &'a str,
    generation: u32,
    timestamp: Option<&'a str>,
}

/// Subagent lifecycle reports, from the parent session or a subagent's own.
fn update_subagent_lifecycle(
    store: &impl StorePort,
    context: &Context<'_>,
    state: &mut State,
    owner: Scope<'_>,
    lifecycle: Lifecycle,
    changes: &mut Changes,
) -> Result<()> {
    match lifecycle {
        Lifecycle::Spawned(spawned) => {
            let hidden = state.phase == Phase::Loading && state.replay_continuation_turn;
            subagents::spawned(
                store,
                context.pane,
                context.generation,
                state,
                spawned,
                subagents::Spawn {
                    owner,
                    hidden,
                    timestamp: context.timestamp,
                },
                changes,
            )
        }
        Lifecycle::Finished(session, outcome) => subagents::finished(
            store,
            context.pane,
            state,
            &session,
            outcome,
            context.timestamp,
            changes,
        ),
    }
}

/// A subagent's own activity stays under its row. Its accounting, mode, and
/// configuration are its own, not the pane's, and its prompt is the row's task.
fn update_in_subagent(
    store: &impl StorePort,
    context: &Context<'_>,
    state: &mut State,
    subagent: &str,
    update: Update,
    changes: &mut Changes,
) -> Result<bool> {
    let Context {
        pane, generation, ..
    } = *context;
    match update {
        Update::Message {
            thought,
            content,
            message_id,
        } => {
            let text = text_block(&content)?;
            let written = transcript::append_text(
                store,
                pane,
                state,
                generation,
                thought,
                transcript::TextChunk {
                    text,
                    message_id: message_id.as_deref(),
                    subagent: Some(subagent),
                },
                changes,
            )?;
            if let Some(seq) = written.filter(|_| !thought) {
                subagents::wrote(state, subagent, seq);
            }
        }
        Update::Tool { update, fields } => update_tool(
            store,
            pane,
            generation,
            state,
            ToolUpdate {
                update,
                fields: &fields,
                subagent: Some(subagent),
            },
            changes,
        )?,
        Update::Plan(entries) => {
            update_plan(pane, generation, state, entries, Some(subagent), changes)?
        }
        Update::Subagent(lifecycle) => update_subagent_lifecycle(
            store,
            context,
            state,
            Scope::Subagent(subagent),
            lifecycle,
            changes,
        )?,
        Update::UserMessage { .. }
        | Update::Usage(_)
        | Update::Configuration(_)
        | Update::Mode(_) => return Ok(false),
    }
    Ok(true)
}

fn text_block(content: &Value) -> Result<&str> {
    if content["type"] != "text" {
        return Err(errors::invalid("This pane accepts text response blocks"));
    }
    content["text"]
        .as_str()
        .ok_or_else(|| errors::invalid("Text response has no text"))
}

pub fn update(
    store: &impl StorePort,
    pane: &str,
    generation: u32,
    state: &mut State,
    params: &Value,
    timestamp: &str,
    changes: &mut Changes,
) -> Result<bool> {
    let probing = matches!(
        state.phase,
        Phase::ProbeLoad | Phase::ProbeList | Phase::ProbeConfig
    );
    let expected = if probing {
        state.probe_session_id.as_deref()
    } else {
        state.session_id.as_deref()
    };
    let Some(expected) = expected else {
        return Err(errors::invalid("Update arrived before a session identity"));
    };
    let update = session::update(
        |session| session == expected || (!probing && state.subagents.contains_key(session)),
        params,
    )
    .map_err(native_error)?;
    if probing {
        return Ok(false);
    }
    let Some(update) = update else {
        return Ok(false);
    };
    let session = params["sessionId"].as_str().unwrap_or_default();
    let context = Context {
        pane,
        generation,
        timestamp: (state.phase != Phase::Loading).then_some(timestamp),
    };
    if state.phase == Phase::Loading && replay_hides(store, &context, state, &update, changes)? {
        return Ok(false);
    }
    match subagents::scope(state, session) {
        Scope::Parent => update_in_parent(store, &context, state, update, changes),
        Scope::Hidden => Ok(false),
        Scope::Subagent(subagent) => {
            update_in_subagent(store, &context, state, subagent, update, changes)
        }
    }
}

/// Native replay: each change of speaker ends the previous one's open items.
/// Returns whether the update belongs to the hidden continuation turn.
fn replay_hides(
    store: &impl StorePort,
    context: &Context<'_>,
    state: &mut State,
    update: &Update,
    changes: &mut Changes,
) -> Result<bool> {
    let Context {
        pane, generation, ..
    } = *context;
    changes.replay_messages = true;
    let role = match update {
        Update::Message { thought: true, .. } => "thought",
        Update::Message { .. } => "message",
        Update::UserMessage { .. } => "user",
        _ => "other",
    };
    if state.replay_role.as_deref() != Some(role) {
        finish_replayed_user(store, pane, generation, state, changes)?;
        transcript::complete(store, pane, state, changes)?;
        state.replay_role = Some(role.into());
    }
    if role == "user" {
        state.replay_continuation_turn = false;
        return Ok(false);
    }
    Ok(state.replay_continuation_turn
        && matches!(
            update,
            Update::Message { .. } | Update::Tool { .. } | Update::Plan(_)
        ))
}

fn update_in_parent(
    store: &impl StorePort,
    context: &Context<'_>,
    state: &mut State,
    update: Update,
    changes: &mut Changes,
) -> Result<bool> {
    let Context {
        pane, generation, ..
    } = *context;
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
            transcript::append_text(
                store,
                pane,
                state,
                generation,
                thought,
                transcript::TextChunk {
                    text: text_block(&content)?,
                    message_id: message_id.as_deref(),
                    subagent: None,
                },
                changes,
            )?;
            true
        }
        Update::Tool { update, fields } => {
            update_tool(
                store,
                pane,
                generation,
                state,
                ToolUpdate {
                    update,
                    fields: &fields,
                    subagent: None,
                },
                changes,
            )?;
            true
        }
        Update::Plan(entries) => {
            update_plan(pane, generation, state, entries, None, changes)?;
            true
        }
        Update::Subagent(lifecycle) => {
            update_subagent_lifecycle(store, context, state, Scope::Parent, lifecycle, changes)?;
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
