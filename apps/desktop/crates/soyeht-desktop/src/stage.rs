//! What the window keeps of how the panes lie on the stage: the sideways scrolls, the tab and
//! the pane to bring into view, the signature the panes' widths depend on, and the pointer's
//! drag of a divider or a pane.

use crate::layout_edit::Gestures;
use gpui::ScrollHandle;

pub(crate) struct Stage {
    /// The sideways scroll of the stage, when the panes need more room than the window has.
    pub(crate) scroll: ScrollHandle,
    /// The sideways scroll of the workspace tabs.
    pub(crate) tab_scroll: ScrollHandle,
    /// The workspace whose tab was last scrolled into view.
    pub(crate) revealed_tab: Option<String>,
    /// How many tabs were not wholly in view when the last frame was laid out.
    pub(crate) hidden_tabs: usize,
    /// The pane to bring into view on the next frame, once the stage has been laid out.
    pub(crate) reveal: Option<String>,
    /// What decides how wide the panes are besides the size of their own card: the window, the
    /// rail and the workspace's arrangement.
    pub(crate) layout_signature: u64,
    /// The divider or the pane being dragged.
    pub(crate) gestures: Gestures,
}

impl Stage {
    pub(crate) fn new() -> Self {
        Self {
            scroll: ScrollHandle::new(),
            tab_scroll: ScrollHandle::new(),
            revealed_tab: None,
            hidden_tabs: 0,
            reveal: None,
            layout_signature: 0,
            gestures: Gestures::default(),
        }
    }
}
