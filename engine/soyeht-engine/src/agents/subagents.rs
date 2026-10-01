//! Subagents a harness runs inside one pane. Each one is a transcript row
//! (`kind: subagent`) whose activity is kept under it by `subagentId`, so a
//! subagent's tool calls never read as the parent agent's own.
use super::{Changes, state::State, transcript};
use crate::{
    errors::{self, Result},
    ports::StorePort,
};
use serde::{Deserialize, Serialize};
use soyeht_acp::subagents::{Outcome, Report, Spawned};
use soyeht_protocol::{SubagentRun, SubagentStatus, TranscriptKind, TranscriptRole};

/// Finished runs are forgotten oldest first beyond this many tracked runs.
const TRACKED_RUNS: usize = 128;
/// The collapsed row previews the start of the subagent's last message.
const SUMMARY_BYTES: usize = 1024;

/// Engine bookkeeping for one reported subagent session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Tracked {
    /// The row's transcript sequence. Absent when the run belongs to a replayed
    /// turn the transcript hides, whose activity is then hidden too.
    row: Option<u64>,
    /// The subagent's latest assistant message, previewed when it finishes.
    last_message: Option<u64>,
    finished: bool,
}

/// Where a session update belongs.
pub(crate) enum Scope<'a> {
    Parent,
    Subagent(&'a str),
    Hidden,
}

pub(crate) fn scope<'a>(state: &State, session: &'a str) -> Scope<'a> {
    match state.subagents.get(session) {
        None => Scope::Parent,
        Some(tracked) if tracked.row.is_none() => Scope::Hidden,
        Some(_) => Scope::Subagent(session),
    }
}

/// Whether a tool call is a collaboration wait on this pane's subagents. Their
/// rows already show that they run, so the wait folds into them instead of
/// reading as a tool of its own. A wait naming any session the pane does not
/// track, or made while it tracks none, stays an ordinary tool row.
pub(crate) fn awaits_subagents(state: &State, tool: &serde_json::Value) -> bool {
    soyeht_acp::subagents::collaboration_wait(tool).is_some_and(|receivers| {
        if receivers.is_empty() {
            !state.subagents.is_empty()
        } else {
            receivers
                .iter()
                .all(|session| state.subagents.contains_key(session))
        }
    })
}

/// The reported subagent session a native request comes from, if it is not
/// the pane's own session.
pub(crate) fn owner<'a>(state: &State, session: Option<&'a str>) -> Option<&'a str> {
    session.filter(|session| state.subagents.contains_key(*session))
}

fn status(outcome: Outcome) -> SubagentStatus {
    match outcome {
        Outcome::Completed => SubagentStatus::Completed,
        Outcome::Failed => SubagentStatus::Failed,
        Outcome::Cancelled => SubagentStatus::Cancelled,
        Outcome::Disconnected => SubagentStatus::Disconnected,
    }
}

fn forget_oldest_finished(state: &mut State) {
    while state.subagents.len() >= TRACKED_RUNS {
        let Some(oldest) = state
            .subagents
            .iter()
            .filter(|(_, tracked)| tracked.finished)
            .min_by_key(|(_, tracked)| tracked.row)
            .map(|(id, _)| id.clone())
        else {
            return;
        };
        state.subagents.remove(&oldest);
    }
}

pub(crate) struct Spawn<'a> {
    pub(crate) owner: Scope<'a>,
    pub(crate) hidden: bool,
    pub(crate) timestamp: Option<&'a str>,
}

pub(crate) fn spawned(
    store: &impl StorePort,
    pane: &str,
    generation: u32,
    state: &mut State,
    spawned: Spawned,
    context: Spawn<'_>,
    changes: &mut Changes,
) -> Result<()> {
    if state.subagents.contains_key(&spawned.session_id) {
        return Err(errors::invalid("Subagent session was reported twice"));
    }
    forget_oldest_finished(state);
    if state.subagents.len() >= TRACKED_RUNS {
        return Err(errors::invalid("Too many subagents are running"));
    }
    if context.hidden || matches!(context.owner, Scope::Hidden) {
        state.subagents.insert(
            spawned.session_id,
            Tracked {
                row: None,
                last_message: None,
                finished: false,
            },
        );
        return Ok(());
    }
    let owner = match context.owner {
        Scope::Subagent(owner) => Some(owner.to_owned()),
        Scope::Parent | Scope::Hidden => None,
    };
    transcript::end_text(store, pane, state, owner.as_deref(), changes)?;
    let mut item = transcript::new_message(
        state,
        generation,
        TranscriptRole::Assistant,
        TranscriptKind::Subagent,
        String::new(),
        false,
    )?;
    item.subagent_id = owner;
    item.subagent = Some(SubagentRun {
        subagent_id: spawned.session_id.clone(),
        name: spawned.name,
        task: spawned.task,
        status: SubagentStatus::Running,
        model: None,
        started_at: context.timestamp.map(Into::into),
        finished_at: None,
        summary: None,
    });
    state.subagents.insert(
        spawned.session_id,
        Tracked {
            row: Some(transcript::sequence(&item.seq)?),
            last_message: None,
            finished: false,
        },
    );
    transcript::put(changes, pane, item, "subagent")
}

/// Applies `change` to a tracked run's row and publishes it.
fn change_row(
    store: &impl StorePort,
    pane: &str,
    state: &State,
    session: &str,
    changes: &mut Changes,
    change: impl FnOnce(&mut SubagentRun),
) -> Result<bool> {
    let Some(tracked) = state.subagents.get(session) else {
        return Ok(false);
    };
    let Some(row) = tracked.row else {
        return Ok(true);
    };
    let mut item = errors::invariant(
        transcript::message(store, pane, row)?,
        "Subagent row disappeared",
    )?;
    let run = errors::invariant(item.subagent.as_mut(), "Subagent row has no run")?;
    change(run);
    item.complete = run.status != SubagentStatus::Running;
    transcript::put(changes, pane, item, "subagent")?;
    Ok(true)
}

pub(crate) fn finished(
    store: &impl StorePort,
    pane: &str,
    state: &mut State,
    session: &str,
    outcome: Outcome,
    timestamp: Option<&str>,
    changes: &mut Changes,
) -> Result<()> {
    let tracked = state
        .subagents
        .get_mut(session)
        .ok_or_else(|| errors::invalid("State update names an unknown subagent"))?;
    tracked.finished = true;
    let summary = tracked
        .last_message
        .map(|seq| transcript::message(store, pane, seq))
        .transpose()?
        .flatten()
        .map(|message| preview(&message.text))
        .filter(|summary| !summary.is_empty());
    change_row(store, pane, state, session, changes, |run| {
        run.status = status(outcome);
        run.finished_at = timestamp.map(Into::into);
        run.summary = summary;
    })?;
    Ok(())
}

fn preview(text: &str) -> String {
    let text = text.trim();
    let mut end = text.len().min(SUMMARY_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].into()
}

/// Folds what the spawning tool call reports about a run into its row.
/// Returns false when the report names no tracked run.
pub(crate) fn reported(
    store: &impl StorePort,
    pane: &str,
    state: &State,
    report: Report,
    changes: &mut Changes,
) -> Result<bool> {
    change_row(store, pane, state, &report.session_id, changes, |run| {
        if report.model.is_some() {
            run.model = report.model;
        }
    })
}

/// Records a subagent's latest assistant message for its finished preview.
pub(crate) fn wrote(state: &mut State, session: &str, seq: u64) {
    if let Some(tracked) = state.subagents.get_mut(session) {
        tracked.last_message = Some(seq);
    }
}

/// The harness process or its session ended: runs it never finished can no
/// longer report, so their rows stop showing them as running.
pub(crate) fn disconnect_all(
    store: &impl StorePort,
    pane: &str,
    state: &mut State,
    changes: &mut Changes,
) -> Result<()> {
    let running = state
        .subagents
        .iter()
        .filter(|(_, tracked)| !tracked.finished)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    for session in running {
        finished(
            store,
            pane,
            state,
            &session,
            Outcome::Disconnected,
            None,
            changes,
        )?;
    }
    state.subagents.clear();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use soyeht_protocol::HarnessLaunch;

    fn wait(receivers: &[&str]) -> serde_json::Value {
        json!({"toolCallId":"call_1","title":"wait","_meta":{"codex":{"collaboration":
            {"tool":"wait","senderThreadId":"parent","receiverThreadIds":receivers}}}})
    }

    #[test]
    fn only_waits_on_tracked_subagents_fold_into_their_rows() {
        let launch = HarnessLaunch {
            id: "registry.codex-acp".into(),
            name: "Codex".into(),
            command: "codex".into(),
            args: vec![],
            version: "1".into(),
            environment: None,
        };
        let mut state = State::new(launch, "digest".into(), None).unwrap();
        // Before any subagent runs, a wait is the agent's own tool call.
        assert!(!awaits_subagents(&state, &wait(&[])));
        state.subagents.insert(
            "child".into(),
            Tracked {
                row: Some(3),
                last_message: None,
                finished: true,
            },
        );
        assert!(awaits_subagents(&state, &wait(&[])));
        assert!(awaits_subagents(&state, &wait(&["child"])));
        assert!(!awaits_subagents(&state, &wait(&["child", "stranger"])));
        assert!(!awaits_subagents(
            &state,
            &json!({"toolCallId":"call_2","title":"wait"})
        ));
    }
}
