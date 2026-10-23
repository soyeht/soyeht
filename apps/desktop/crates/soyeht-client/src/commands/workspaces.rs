//! The requests that make a workspace of a folder, name it, arrange its panes and close it.

use super::{Command, key};
use soyeht_protocol::{
    Request, WorkspaceCloseParams, WorkspaceCreateParams, WorkspaceLayoutSetParams,
    WorkspaceRenameParams,
};

/// The request a command of a workspace needs, or `None` when it is not one of these.
pub(super) fn request(command: &Command) -> Option<Request> {
    Some(match command {
        Command::SetLayout {
            workspace_id,
            layout,
        } => Request::WorkspaceLayoutSet(WorkspaceLayoutSetParams {
            workspace_id: workspace_id.clone(),
            layout: layout.clone(),
            idempotency_key: key(),
        }),
        Command::CloseWorkspace { workspace_id } => Request::WorkspaceClose(WorkspaceCloseParams {
            workspace_id: workspace_id.clone(),
            idempotency_key: key(),
        }),
        Command::CreateWorkspace { name, path, .. } => {
            Request::WorkspaceCreate(WorkspaceCreateParams {
                name: name.clone(),
                path: path.clone(),
                idempotency_key: key(),
            })
        }
        Command::RenameWorkspace { workspace_id, name } => {
            Request::WorkspaceRename(WorkspaceRenameParams {
                workspace_id: workspace_id.clone(),
                name: name.clone(),
                idempotency_key: key(),
            })
        }
        _ => return None,
    })
}
