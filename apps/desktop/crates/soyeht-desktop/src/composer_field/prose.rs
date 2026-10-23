//! What makes a composer field a field of prose, a text pane's text: Return adds a line, the
//! text is drawn in the looks its Markdown gives, the keys of the formats and of saving are the
//! pane's, the caret is a line, and the pane around the field scrolls to it.

use super::{ComposerField, chord_of};
use crate::theme::{Palette, desktop};
use gpui::{
    Bounds, Context, EventEmitter, HighlightStyle, Keystroke, Pixels, ScrollHandle, Window, point,
    px, size,
};
use soyeht_client::{
    editor::keys::{Action, action},
    writing::{rules::format_shortcut, screenplay::LineStyle},
};
use soyeht_protocol::WritingFormatKind;
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    ops::Range,
};

/// The parts of a text that have a look, each as a range of its bytes with how it is drawn.
pub type Looks = Vec<(Range<usize>, HighlightStyle)>;

/// What a field that holds prose tells the pane that owns it, besides the keys it takes itself.
#[derive(Debug, Clone, PartialEq)]
pub enum ProseEvent {
    /// The text or the selection changed.
    Edited,
    /// A key that formats the selected text was pressed.
    Format(WritingFormatKind),
    /// The key that saves was pressed.
    Save,
    /// Return (or Tab, when `tab`) was pressed in the page of a screenplay: it moves on to the
    /// next element.
    Advance { tab: bool },
}

impl EventEmitter<ProseEvent> for ComposerField {}

/// Return or Tab on its own in the page of a screenplay.
fn advance_key(keystroke: &Keystroke) -> Option<ProseEvent> {
    let modifiers = &keystroke.modifiers;
    if modifiers.alt || modifiers.shift || modifiers.control || modifiers.platform {
        return None;
    }
    match keystroke.key.as_str() {
        "enter" => Some(ProseEvent::Advance { tab: false }),
        "tab" => Some(ProseEvent::Advance { tab: true }),
        _ => None,
    }
}

/// The parts of a field that holds prose (a writing pane's text) rather than a message: Return
/// adds a line, the text is drawn in the looks its Markdown gives, and the pane around it
/// scrolls to the caret.
pub(super) struct Prose {
    /// The parts of a text that have a look.
    looks: fn(&str, &Palette) -> Looks,
    /// The scroll of the pane around the field, which follows the caret.
    scroll: ScrollHandle,
    /// The caret moved: the pane around the field is scrolled to it once it is laid out.
    reveal: bool,
    /// The looks of the text last drawn, with what they were made for.
    drawn: Option<(u64, Looks)>,
    /// The text is the page of a screenplay: the style of each of its lines.
    page: Option<Vec<LineStyle>>,
}

/// Where the pane around a field of prose scrolls to, to show the caret.
pub(super) struct Reveal {
    scroll: ScrollHandle,
    /// The caret, in the window, and the height of its line.
    at: gpui::Point<Pixels>,
    line: Pixels,
}

impl Reveal {
    /// Scrolls the least that brings the caret's line into the pane, with a line of room.
    pub(super) fn apply(self, window: &mut Window) {
        let view = self.scroll.bounds();
        let (top, bottom) = (view.top() + self.line, view.bottom() - self.line);
        let shift = if self.at.y < top {
            top - self.at.y
        } else if self.at.y + self.line > bottom {
            bottom - (self.at.y + self.line)
        } else {
            return;
        };
        let offset = self.scroll.offset();
        self.scroll
            .set_offset(point(offset.x, (offset.y + shift).min(px(0.0))));
        window.refresh();
    }
}

impl ComposerField {
    /// A field for prose: `looks` says which parts of a text have a look, and `scroll` is the
    /// scroll of the pane around the field.
    pub fn prose(
        palette: &'static Palette,
        looks: fn(&str, &Palette) -> Looks,
        scroll: ScrollHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            prose: Some(Prose {
                looks,
                scroll,
                reveal: false,
                drawn: None,
                page: None,
            }),
            ..Self::new(palette, cx)
        }
    }

    /// A field for the page of a screenplay: prose with no Markdown looks, whose lines are drawn
    /// as `set_page` says.
    pub fn screenplay(
        palette: &'static Palette,
        scroll: ScrollHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut field = Self::prose(palette, |_, _| Vec::new(), scroll, cx);
        if let Some(prose) = &mut field.prose {
            prose.page = Some(Vec::new());
        }
        field
    }

    /// The style of each line of the page, for the next frame.
    pub fn set_page(&mut self, styles: Vec<LineStyle>) {
        if let Some(prose) = &mut self.prose {
            prose.page = Some(styles);
        }
    }

    /// The style of each line when the text is a screenplay's page.
    pub(super) fn page_styles(&self) -> Option<&[LineStyle]> {
        self.prose.as_ref()?.page.as_deref()
    }

    /// Puts `text` in place of the bytes `range`, the caret after it.
    pub fn replace_range(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) {
        self.draft.place(range.start, false);
        self.draft.place(range.end, true);
        self.draft.insert(text);
        self.goal_x = None;
        self.edited(true, cx);
        cx.notify();
    }

    /// Puts the caret at byte `at`, and scrolls the pane to it.
    pub fn place_caret(&mut self, at: usize, cx: &mut Context<Self>) {
        self.draft.place(at, false);
        self.goal_x = None;
        self.edited(true, cx);
        cx.notify();
    }

    /// Puts the text the engine has in place of what the field holds, without an edit of the
    /// person's.
    pub fn load(&mut self, text: &str, cx: &mut Context<Self>) {
        self.draft.load(text);
        self.goal_x = None;
        cx.notify();
    }

    /// Leaves the selection and puts the caret where it ended.
    pub fn collapse(&mut self, cx: &mut Context<Self>) {
        let end = self.draft.selection().end;
        self.draft.place(end, false);
        self.edited(false, cx);
        cx.notify();
    }

    /// The selection as the bytes it started and ended at.
    pub fn selection_ends(&self) -> (usize, usize) {
        let selected = self.draft.selection();
        let head = self.draft.caret();
        let anchor = if head == selected.start {
            selected.end
        } else {
            selected.start
        };
        (anchor, head)
    }

    /// Tells the pane that owns a field of prose that the text or the selection changed, and
    /// scrolls to the caret.
    pub(super) fn edited(&mut self, reveal: bool, cx: &mut Context<Self>) {
        if let Some(prose) = &mut self.prose {
            prose.reveal |= reveal;
            cx.emit(ProseEvent::Edited);
        }
    }

    /// The keys of prose that are not edits: the formats of the selected text, and saving.
    /// True when the key was one of them.
    pub(super) fn prose_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = &keystroke.modifiers;
        let command = if cfg!(target_os = "macos") {
            modifiers.platform
        } else {
            modifiers.control
        };
        let plain = !modifiers.alt && !modifiers.shift;
        let save = || {
            (plain && action(&chord_of(keystroke)) == Some(Action::Save))
                .then_some(ProseEvent::Save)
        };
        let asked = if self.page_styles().is_some() {
            // The formats are Markdown's: the page of a screenplay has none.
            advance_key(keystroke).or_else(save)
        } else if plain {
            format_shortcut(&keystroke.key, command)
                .map(ProseEvent::Format)
                .or_else(save)
        } else {
            None
        };
        let Some(asked) = asked else {
            return false;
        };
        cx.emit(asked);
        cx.stop_propagation();
        true
    }

    /// The parts of the text that have a look, made again only when the text or the palette
    /// changed.
    pub(super) fn looks(&mut self) -> Looks {
        let Some(prose) = &mut self.prose else {
            return Vec::new();
        };
        let mut hasher = DefaultHasher::new();
        self.draft.text().hash(&mut hasher);
        std::ptr::from_ref(self.palette).hash(&mut hasher);
        let key = hasher.finish();
        if prose.drawn.as_ref().is_none_or(|(known, _)| *known != key) {
            prose.drawn = Some((key, (prose.looks)(self.draft.text(), self.palette)));
        }
        prose
            .drawn
            .as_ref()
            .map(|(_, looks)| looks.clone())
            .unwrap_or_default()
    }

    /// The caret of a field of prose while it has the keyboard: a line as tall as the text's, at
    /// the caret. A message line draws its caret as a block over the character instead, which
    /// cannot show at the end of a line.
    pub(super) fn thin_caret(
        &self,
        bounds: Bounds<Pixels>,
        focused: bool,
    ) -> Option<(Bounds<Pixels>, u32)> {
        if self.prose.is_none()
            || !focused
            || self.draft.has_selection()
            || self.draft.is_composing()
        {
            return None;
        }
        let (origin, height) = match &self.layout {
            Some(layout) => (
                layout.position_for_index(self.draft.caret())?,
                layout.line_height(),
            ),
            None => (bounds.origin, bounds.size.height),
        };
        let width = px(desktop::EDITOR_CARET_WIDTH);
        Some((Bounds::new(origin, size(width, height)), self.palette.ink))
    }

    /// What the paint of a field of prose does when the caret moved: show it.
    pub(super) fn take_reveal(&mut self) -> Option<Reveal> {
        let prose = self.prose.as_mut()?;
        if !std::mem::take(&mut prose.reveal) {
            return None;
        }
        let layout = self.layout.as_ref()?;
        Some(Reveal {
            scroll: prose.scroll.clone(),
            at: layout.position_for_index(self.draft.caret())?,
            line: layout.line_height(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pressed(keys: &str) -> Option<ProseEvent> {
        advance_key(&Keystroke::parse(keys).unwrap())
    }

    #[test]
    fn return_and_tab_on_their_own_move_a_screenplay_on() {
        assert_eq!(pressed("enter"), Some(ProseEvent::Advance { tab: false }));
        assert_eq!(pressed("tab"), Some(ProseEvent::Advance { tab: true }));
    }

    #[test]
    fn a_modified_key_or_another_key_does_not() {
        for keys in ["shift-enter", "shift-tab", "ctrl-enter", "alt-tab", "a"] {
            assert_eq!(pressed(keys), None, "{keys}");
        }
    }
}
