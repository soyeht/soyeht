//! The requests of what the person does with a running agent: loops, roles, drafts, the queue of
//! waiting messages, the agent's questions and sign-ins.

use super::{Command, key};
use soyeht_protocol::{
    LoopSetParams, PaneAuthCancelParams, PaneAuthStartParams, PaneDraftParams,
    PaneElicitationAnswerParams, PaneQueueRemoveParams, PaneQueueResumeParams, PaneRoleSetParams,
    Request,
};

/// The request a command of an agent needs, or `None` when it is not one of these.
pub(super) fn request(command: &Command) -> Option<Request> {
    Some(match command {
        Command::SetLoop {
            pane_id,
            prompt,
            interval,
            max_rounds,
            stop_when,
            enabled,
        } => Request::LoopSet(LoopSetParams {
            pane_id: pane_id.clone(),
            prompt: prompt.clone(),
            interval: *interval,
            max_rounds: *max_rounds,
            stop_when: stop_when.clone(),
            enabled: *enabled,
            idempotency_key: key(),
        }),
        Command::SetRole { pane_id, role } => Request::PaneRoleSet(PaneRoleSetParams {
            pane_id: pane_id.clone(),
            role: role.clone(),
            idempotency_key: key(),
        }),
        Command::SetDraft { pane_id, active } => Request::PaneDraft(PaneDraftParams {
            pane_id: pane_id.clone(),
            active: *active,
            idempotency_key: key(),
        }),
        Command::RemoveQueued { pane_id, prompt_id } => {
            Request::PaneQueueRemove(PaneQueueRemoveParams {
                pane_id: pane_id.clone(),
                prompt_id: prompt_id.clone(),
                idempotency_key: key(),
            })
        }
        Command::ResumeQueue { pane_id } => Request::PaneQueueResume(PaneQueueResumeParams {
            pane_id: pane_id.clone(),
            idempotency_key: key(),
        }),
        Command::AnswerElicitation {
            pane_id,
            request_id,
            fields,
            cancel,
        } => Request::PaneElicitationAnswer(PaneElicitationAnswerParams {
            pane_id: pane_id.clone(),
            request_id: request_id.clone(),
            fields: fields.clone(),
            cancel: cancel.then_some(true),
            idempotency_key: key(),
        }),
        Command::StartSignIn { pane_id, method_id } => {
            Request::PaneAuthStart(PaneAuthStartParams {
                pane_id: pane_id.clone(),
                auth_method_id: method_id.clone(),
                idempotency_key: key(),
            })
        }
        Command::CancelSignIn { pane_id } => Request::PaneAuthCancel(PaneAuthCancelParams {
            pane_id: pane_id.clone(),
            idempotency_key: key(),
        }),
        _ => return None,
    })
}
