//! A prompt turn that the agent could not complete. The turn ends with the
//! `error` stop reason and the transcript says why in a system message. When
//! the provider refused the pane's model, the refusal is also remembered for
//! `model.list`, whether the agent reported it as a protocol error or only as
//! the text of its reply.

use super::{Changes, state::State, transcript};
use crate::{
    errors::{self, Result},
    model::StoredPane,
    ports::StorePort,
};
use soyeht_protocol::{TranscriptKind, TranscriptMessage, TranscriptRole, limits};
use soyeht_store::agents::AgentModelRefusal;

/// How the agent reported the failure.
pub(super) enum Failure {
    /// A protocol error with this code.
    Rpc(i32),
    /// A reply that the provider's refusal of the model wrote as ordinary text.
    RejectedModelReply,
}

/// The text of the one reply a finished turn wrote, when it is the provider's
/// refusal of the pane's model. The turn must hold nothing else besides the
/// person's prompt, its thoughts and its usage (no earlier message, tool call
/// or plan), and the reply
/// must be the refusal sentence for the selected model, not a quote of it.
pub(super) fn rejected_model_reply(
    store: &impl StorePort,
    pane: &str,
    state: &State,
) -> Result<Option<String>> {
    let (Some(active), Some(model)) = (state.active.as_ref(), selected_model(state)) else {
        return Ok(None);
    };
    let page = store
        .agent_transcript(pane, None, limits::TRANSCRIPT_PAGE_MESSAGES)
        .map_err(errors::storage)?;
    let in_turn = |message: &&TranscriptMessage| {
        message.prompt_id.as_deref() == Some(active.prompt_id.as_str())
    };
    // The page ends inside this turn: the rest of the turn is unknown.
    if page.next_cursor.is_some() && page.items.first().is_some_and(|m| in_turn(&m)) {
        return Ok(None);
    }
    let mut written = page.items.iter().filter(in_turn).filter(|message| {
        message.role != TranscriptRole::User
            && !matches!(
                message.kind,
                TranscriptKind::Thought | TranscriptKind::Usage
            )
    });
    let (Some(reply), None) = (written.next(), written.next()) else {
        return Ok(None);
    };
    Ok((reply.role == TranscriptRole::Assistant
        && reply.kind == TranscriptKind::Message
        && states_model_refusal(reply.text.trim(), model))
    .then(|| reply.text.trim().to_owned()))
}

/// The provider's sentence for a model it will not serve, as the whole reply.
/// Anything before it, after it or in place of the model id is not the refusal.
fn states_model_refusal(text: &str, model: &str) -> bool {
    const LONGEST_BYTES: usize = 400;
    const OPENING: &str = " an issue with the selected model (";
    const CLOSING: &str = "). It may not exist or you may not have access to it";
    let Some(rest) = text
        .strip_prefix("There's")
        .or_else(|| text.strip_prefix("There\u{2019}s"))
        .and_then(|rest| rest.strip_prefix(OPENING))
    else {
        return false;
    };
    text.len() <= LONGEST_BYTES
        && rest
            .strip_prefix(model)
            .is_some_and(|rest| rest.starts_with(CLOSING))
}

/// Records why the turn failed.
pub(super) fn report(
    store: &impl StorePort,
    saved: &StoredPane,
    state: &mut State,
    changes: &mut Changes,
    clean: &str,
    failure: Failure,
    timestamp: &str,
) -> Result<()> {
    let refused_model = match failure {
        Failure::Rpc(code) => observed_model_refusal(code, clean),
        Failure::RejectedModelReply => true,
    }
    .then(|| selected_model(state).map(str::to_owned))
    .flatten();
    let text = if let Some(model) = refused_model {
        if let Some(reason) = bounded_refusal_reason(clean) {
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
    // Some adapters emit a protocol error as an assistant chunk before returning
    // the same typed error. Keep one visible copy for this turn. A refusal that
    // arrived only as a reply is not typed yet, so it always gets its own notice.
    let shown = matches!(failure, Failure::Rpc(_))
        && assistant_already_reported_error(store, &saved.pane.pane_id, state, clean)?;
    if !shown {
        changes.messages.push(transcript::new_message(
            state,
            saved.pane.generation,
            TranscriptRole::System,
            TranscriptKind::System,
            text,
            true,
        )?);
    }
    Ok(())
}

pub(super) fn clean_agent_error(message: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::{clean_agent_error, observed_model_refusal, states_model_refusal};

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

    #[test]
    fn a_reply_is_a_model_refusal_only_when_it_is_the_whole_sentence_for_the_selected_model() {
        let refusal = "There's an issue with the selected model (m-1). It may not exist or you may not have access to it. Run /model to pick a different model.";
        assert!(states_model_refusal(refusal, "m-1"));
        assert!(states_model_refusal(
            &refusal.replace('\'', "\u{2019}"),
            "m-1"
        ));
        // Another model than the pane's.
        assert!(!states_model_refusal(refusal, "m-2"));
        assert!(!states_model_refusal(refusal, "m"));
        // The sentence quoted or explained inside a reply.
        let quoted = format!("The message \"{refusal}\" means the account lacks the model.");
        assert!(!states_model_refusal(&quoted, "m-1"));
        assert!(!states_model_refusal(&format!("It says: {refusal}"), "m-1"));
        // The same opening in a long reply, or without the second sentence.
        let long = format!("{refusal} {}", "More. ".repeat(100));
        assert!(!states_model_refusal(&long, "m-1"));
        assert!(!states_model_refusal(
            "There's an issue with the selected model (m-1), says a log.",
            "m-1"
        ));
        assert!(!states_model_refusal("Done.", "m-1"));
    }
}
