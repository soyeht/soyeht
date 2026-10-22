//! An arrangement the person is dragging toward: shown in place of the stored one while a
//! divider moves, and stored once it lets go (the Mac's layout draft: `beginDraft`,
//! `updateDraft`, `commitDraft`).

use super::{Effect, State};
use crate::layout;
use soyeht_protocol::{Workspace, WorkspacePaneLayoutNode as Node};

/// What the person is told when the engine does not store an arrangement; the Mac's words.
const NOT_SAVED: &str = "Couldn't save the layout. The last saved arrangement is back.";

/// The tree a workspace is being dragged to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LayoutDraft {
    pub(crate) workspace_id: String,
    pub(crate) tree: Node,
}

impl State {
    /// Shows `tree` for `workspace_id` until the draft is stored or dropped.
    pub fn draft_layout(&mut self, workspace_id: &str, tree: Node) {
        self.layout_draft = Some(LayoutDraft {
            workspace_id: workspace_id.to_owned(),
            tree,
        });
    }

    /// Drops the draft: the stored arrangement shows again.
    pub fn cancel_layout_draft(&mut self) {
        self.layout_draft = None;
    }

    /// Stores the draft as the arrangement of its workspace, when the workspace still has the
    /// panes the draft arranges and the draft changes something. The change shows at once;
    /// the engine's echo of the stored layout comes a round trip later.
    pub fn commit_layout_draft(&mut self) -> Vec<Effect> {
        let Some(draft) = self.layout_draft.take() else {
            return Vec::new();
        };
        let current = self.layout(&draft.workspace_id, |_| true);
        if current.as_ref() == Some(&draft.tree)
            || current.is_none_or(|tree| !same_panes(&tree, &draft.tree))
        {
            return Vec::new();
        }
        vec![self.store_layout(&draft.workspace_id, draft.tree)]
    }

    /// Remembers the layout the engine sent for `workspace`.
    pub(crate) fn confirm_layout(&mut self, workspace: &Workspace) {
        self.confirmed_layouts
            .insert(workspace.workspace_id.clone(), workspace.layout.clone());
    }

    /// The engine refused the arrangement drawn for `workspace_id`: the last one it confirmed is
    /// drawn again, and the person is told (the Mac's `PaneLayoutController` drops what was
    /// queued and shows its confirmed state).
    pub(crate) fn restore_confirmed_layout(&mut self, workspace_id: &str) {
        self.layout_draft = None;
        if let (Some(confirmed), Some(workspace)) = (
            self.confirmed_layouts.get(workspace_id),
            self.workspaces
                .iter_mut()
                .find(|workspace| workspace.workspace_id == workspace_id),
        ) {
            workspace.layout = confirmed.clone();
        }
        self.problem = Some(NOT_SAVED.to_owned());
    }
}

/// Whether two arrangements place the same panes.
pub(crate) fn same_panes(first: &Node, second: &Node) -> bool {
    let (mut first, mut second) = (layout::pane_ids(first), layout::pane_ids(second));
    first.sort_unstable();
    second.sort_unstable();
    first == second
}
