//! The requests of a pane: its size, focus, prompts, transcript, options and permissions.

use super::{Command, key};
use soyeht_protocol::{
    ConfigValue, PaneCancelParams, PaneCloseParams, PaneCommandsListParams, PaneConfigSetParams,
    PaneCreateParams, PaneFocusParams, PaneHarnessSwitchParams, PaneMode,
    PanePermissionAnswerParams, PanePromptParams, PaneRenameParams, PaneResizeParams,
    PaneStreamSubscribeParams, PaneTranscriptParams, PromptOrigin, ReplayFrom, Request,
    TailSelector, UsageListParams, client_policy,
};

/// The request a command of a pane needs, or `None` when it is not one of these.
pub(super) fn request(command: &Command) -> Option<Request> {
    Some(match command {
        Command::Resize {
            pane_id,
            rows,
            cols,
        } => Request::PaneResize(PaneResizeParams {
            pane_id: pane_id.clone(),
            cols: u32::from(*cols),
            rows: u32::from(*rows),
            idempotency_key: key(),
        }),
        Command::Subscribe { pane_id } => Request::PaneStreamSubscribe(PaneStreamSubscribeParams {
            pane_id: pane_id.clone(),
            from: ReplayFrom::V0(TailSelector::Tail),
        }),
        Command::Focus { pane_id } => Request::PaneFocus(PaneFocusParams {
            pane_id: pane_id.clone(),
            idempotency_key: key(),
        }),
        Command::CreateTerminal {
            workspace_id, cwd, ..
        } => create(workspace_id, PaneMode::Terminal, None, cwd.as_ref()),
        Command::CreateAgent {
            workspace_id,
            harness,
            cwd,
            ..
        } => create(workspace_id, PaneMode::Agent, Some(harness), cwd.as_ref()),
        Command::ClosePane { pane_id } => Request::PaneClose(PaneCloseParams {
            pane_id: pane_id.clone(),
            idempotency_key: key(),
        }),
        Command::Prompt {
            pane_id,
            text,
            attachments,
        } => Request::PanePrompt(PanePromptParams {
            pane_id: pane_id.clone(),
            text: text.clone(),
            origin: Some(PromptOrigin::Human),
            attachments: (!attachments.is_empty())
                .then(|| attachments.iter().map(|draft| draft.wire().0).collect()),
            idempotency_key: key(),
        }),
        Command::Cancel { pane_id } => Request::PaneCancel(PaneCancelParams {
            pane_id: pane_id.clone(),
            idempotency_key: key(),
        }),
        Command::LoadTranscript { pane_id } => transcript_page(pane_id, None),
        Command::LoadOlder { pane_id, before } => transcript_page(pane_id, Some(before)),
        Command::LoadCommands { pane_id } => Request::PaneCommandsList(PaneCommandsListParams {
            pane_id: pane_id.clone(),
        }),
        Command::LoadUsage => Request::UsageList(UsageListParams {}),
        Command::SetConfig {
            pane_id,
            config_id,
            value,
        } => Request::PaneConfigSet(PaneConfigSetParams {
            pane_id: pane_id.clone(),
            config_id: config_id.clone(),
            value: ConfigValue::V0(value.clone()),
            idempotency_key: key(),
        }),
        Command::SwitchHarness {
            pane_id,
            harness,
            model,
            effort,
        } => Request::PaneHarnessSwitch(PaneHarnessSwitchParams {
            pane_id: pane_id.clone(),
            harness: harness.clone(),
            model: model.clone(),
            effort: effort.clone(),
            idempotency_key: key(),
        }),
        Command::AnswerPermission {
            pane_id,
            request_id,
            option_id,
        } => Request::PanePermissionAnswer(PanePermissionAnswerParams {
            pane_id: pane_id.clone(),
            request_id: request_id.clone(),
            option_id: option_id.clone(),
            idempotency_key: key(),
        }),
        Command::RenamePane { pane_id, name } => Request::PaneRename(PaneRenameParams {
            pane_id: pane_id.clone(),
            name: name.clone(),
            idempotency_key: key(),
        }),
        // Input is sent in pieces by the caller, and the queue comes in pages: the engine fetches
        // them one after the other.
        _ => return None,
    })
}

fn transcript_page(pane_id: &str, before: Option<&String>) -> Request {
    Request::PaneTranscript(PaneTranscriptParams {
        pane_id: pane_id.to_owned(),
        before: before.cloned(),
        limit: Some(client_policy::PAGE_SIZE_TRANSCRIPT),
    })
}

fn create(
    workspace_id: &str,
    mode: PaneMode,
    harness: Option<&String>,
    cwd: Option<&String>,
) -> Request {
    Request::PaneCreate(PaneCreateParams {
        workspace_id: workspace_id.to_owned(),
        mode,
        cwd: cwd.cloned(),
        name: None,
        cols: None,
        rows: None,
        idempotency_key: key(),
        harness: harness.cloned(),
        model: None,
        effort: None,
        permission_mode: None,
        url: None,
        incognito: None,
        path: None,
        writing_kind: None,
        writing_section: None,
    })
}
