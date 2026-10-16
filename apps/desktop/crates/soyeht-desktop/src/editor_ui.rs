//! What the window keeps for an editor pane besides what the engine says about its file: how far
//! the text is scrolled, where it was last drawn, the find bar, and the scroll of the tabs and of
//! the project panel. It hangs off the pane's `PaneUi`, so it goes when the pane closes.

use crate::theme::editor;
use gpui::{Bounds, Pixels, ScrollHandle};

/// The find bar of an editor: the words being searched for, and the toggles.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct FindBar {
    pub open: bool,
    /// Whether typing goes to the bar rather than to the text.
    pub focused: bool,
    pub query: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

#[derive(Default)]
pub(crate) struct EditorUi {
    /// How far the text is scrolled down and to the right, in points.
    pub top: f32,
    pub left: f32,
    /// Where the text area (lines and gutter) was last drawn, and how many lines fit in it.
    pub bounds: Option<Bounds<Pixels>>,
    pub rows: u32,
    pub columns: u32,
    /// The width of a character and the height of a line, as the text was last drawn.
    pub cell: (f32, f32),
    /// The cells of the longest line in view, to keep the sideways scroll inside the text.
    pub longest: usize,
    /// A drag that selects text is under way.
    pub dragging: bool,
    /// Whether the project panel has the keys that move through it.
    pub tree_focused: bool,
    /// The file the project panel last opened its folders to.
    pub revealed: Option<String>,
    pub find: FindBar,
    pub tabs_scroll: ScrollHandle,
    pub tree_scroll: ScrollHandle,
}

impl EditorUi {
    /// The height of the whole text, with the room above the first line and below the last.
    pub fn content_height(lines: u32) -> f32 {
        lines as f32 * editor::LINE_HEIGHT + editor::VIEWPORT_VERTICAL_INSET
    }

    /// The first line in view.
    pub fn first_line(&self) -> u32 {
        (self.top / editor::LINE_HEIGHT).max(0.0) as u32
    }

    /// The height of the text area, and the width the text has beside the gutter.
    fn extent(&self) -> (f32, f32) {
        let (width, height) = self.bounds.map_or((0.0, 0.0), |bounds| {
            (f32::from(bounds.size.width), f32::from(bounds.size.height))
        });
        (
            height,
            width - editor::GUTTER_WIDTH - 2.0 * editor::TEXT_INSET,
        )
    }

    /// Keeps the scroll inside the text.
    pub fn clamp(&mut self, lines: u32, longest_line_cells: usize) {
        let (viewport, width) = self.extent();
        let height_max = (Self::content_height(lines) - viewport).max(0.0);
        self.top = self.top.clamp(0.0, height_max);
        let width_max = (longest_line_cells as f32 * self.cell.0 - width).max(0.0);
        self.left = self.left.clamp(0.0, width_max);
    }

    /// Scrolls so that the line at `line`, in the cell `cell`, is in view.
    pub fn reveal(&mut self, line: u32, cell: usize) {
        let (height, width) = self.extent();
        let y = line as f32 * editor::LINE_HEIGHT;
        if y < self.top {
            self.top = y;
        } else if y + editor::LINE_HEIGHT + editor::VIEWPORT_VERTICAL_INSET > self.top + height {
            self.top = y + editor::LINE_HEIGHT + editor::VIEWPORT_VERTICAL_INSET - height;
        }
        let x = cell as f32 * self.cell.0;
        if x < self.left {
            self.left = x;
        } else if x + self.cell.0 > self.left + width {
            self.left = x + self.cell.0 - width;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    fn ui() -> EditorUi {
        EditorUi {
            bounds: Some(Bounds::new(
                point(px(0.0), px(0.0)),
                size(px(400.0), px(190.0)),
            )),
            cell: (7.0, editor::LINE_HEIGHT),
            ..EditorUi::default()
        }
    }

    #[test]
    fn the_scroll_stays_inside_the_text() {
        let mut ui = ui();
        ui.top = 10_000.0;
        ui.left = 10_000.0;
        ui.clamp(100, 200);
        assert_eq!(ui.top, EditorUi::content_height(100) - 190.0);
        let width = 400.0 - editor::GUTTER_WIDTH - 2.0 * editor::TEXT_INSET;
        assert_eq!(ui.left, 200.0 * 7.0 - width);
        ui.top = -5.0;
        ui.left = -5.0;
        ui.clamp(100, 200);
        assert_eq!((ui.top, ui.left), (0.0, 0.0));
        ui.clamp(2, 10);
        assert_eq!((ui.top, ui.left), (0.0, 0.0));
    }

    #[test]
    fn revealing_a_line_scrolls_the_least_that_shows_it() {
        let mut ui = ui();
        ui.reveal(3, 0);
        assert_eq!(ui.top, 0.0);
        ui.reveal(40, 0);
        let bottom =
            40.0 * editor::LINE_HEIGHT + editor::LINE_HEIGHT + editor::VIEWPORT_VERTICAL_INSET;
        assert_eq!(ui.top, bottom - 190.0);
        assert_eq!(ui.first_line(), (ui.top / editor::LINE_HEIGHT) as u32);
        ui.reveal(2, 0);
        assert_eq!(ui.top, 2.0 * editor::LINE_HEIGHT);
        ui.reveal(2, 80);
        assert!(ui.left > 0.0);
        ui.reveal(2, 0);
        assert_eq!(ui.left, 0.0);
    }
}
