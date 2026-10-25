//! The text a key types, for the panes that take their keys as events: a terminal and an editor.
//! The platform's text input delivers it to this handler, as it does to the composer, so that a
//! dead key (`~` then `a` is `ã`), AltGr and an input method compose as they do in every other
//! program. The keys that edit, move or act (Control+C, the arrows, Enter, function keys) stay
//! key events (`input::key_down`). A terminal gets the text as the bytes it would be typed as; an
//! editor inserts it (or puts it in the find bar when that has the keys). Both follow the same
//! rule as the keys for which pane is in front (`Workbench::keys_target`).

use super::{Workbench, input::KeyTarget};
use gpui::{
    App, Bounds, Context, ElementInputHandler, EntityInputHandler, FocusHandle, IntoElement,
    Pixels, Point, Styled, UTF16Selection, Window, canvas, point, size,
};
use std::ops::Range;

/// The text an input method is composing, and the pane it is composed in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Preedit {
    pane_id: String,
    text: String,
}

impl Preedit {
    /// What the input method marked in `pane_id`; nothing when it marked no text.
    fn marked(pane_id: &str, text: &str) -> Option<Self> {
        (!text.is_empty()).then(|| Self {
            pane_id: pane_id.to_owned(),
            text: text.to_owned(),
        })
    }

    /// The marked range, in the UTF-16 units the platform counts.
    fn units(&self) -> Range<usize> {
        0..self.text.encode_utf16().count()
    }
}

impl Workbench {
    /// The composition under way in `pane_id`, if there is one.
    pub(crate) fn preedit_of(&self, pane_id: &str) -> Option<&str> {
        self.preedit
            .as_ref()
            .filter(|preedit| preedit.pane_id == pane_id)
            .map(|preedit| preedit.text.as_str())
    }

    /// Drops a composition that has lost its place: the keyboard moved to another pane (the engine
    /// focused one, a tab closed), to a form's field, or to another window. Called every frame.
    pub(crate) fn settle_preedit(&mut self, window: &Window, cx: &App) {
        let Some(preedit) = &self.preedit else {
            return;
        };
        let in_place = window.is_window_active()
            && self
                .keys_target(window, cx)
                .is_some_and(|target| target.pane_id() == preedit.pane_id);
        if !in_place {
            self.preedit = None;
        }
    }

    /// Text typed or committed: it ends the composition, and goes to the pane that has the keys.
    fn type_text(&mut self, text: &str, window: &Window, cx: &mut Context<Self>) {
        // A frame is drawn again only when something that was on screen (the composition) goes.
        if self.preedit.take().is_some() {
            cx.notify();
        }
        if text.is_empty() {
            return;
        }
        match self.keys_target(window, cx) {
            Some(KeyTarget::Terminal(pane_id)) => {
                self.send_input(&pane_id, text.as_bytes().to_vec(), cx);
            }
            Some(KeyTarget::Editor(pane_id)) => self.type_in_editor(&pane_id, text, cx),
            None => {}
        }
    }

    /// Where the caret is, for the input method's candidate window: the cursor of a terminal
    /// (where it would be when the program hides it), the caret of an editor.
    fn caret_area(&self, window: &Window, cx: &App) -> Option<Bounds<Pixels>> {
        match self.keys_target(window, cx)? {
            KeyTarget::Terminal(pane_id) => {
                let (bounds, cell) = self.panes.get(&pane_id)?.terminal.geometry?;
                let cursor = self.state.screen(&pane_id)?.cursor_cell();
                let at: Point<Pixels> = point(
                    bounds.origin.x + cell.width * f32::from(cursor.column),
                    bounds.origin.y + cell.height * f32::from(cursor.row),
                );
                Some(Bounds::new(at, size(cell.width, cell.height)))
            }
            KeyTarget::Editor(pane_id) => self.panes.get(&pane_id)?.editor.caret,
        }
    }
}

impl KeyTarget {
    fn pane_id(&self) -> &str {
        match self {
            Self::Terminal(pane_id) | Self::Editor(pane_id) => pane_id,
        }
    }
}

impl EntityInputHandler for Workbench {
    fn text_for_range(
        &mut self,
        _range: Range<usize>,
        _adjusted: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        // The panes hold no line of text the input method could read back.
        None
    }

    fn selected_text_range(
        &mut self,
        _ignoring_whitespace: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        // The caret is at the end of what is being composed.
        let end = self
            .preedit
            .as_ref()
            .map_or(0, |preedit| preedit.units().end);
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.preedit.as_ref().map(Preedit::units)
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.preedit.take().is_some() {
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        _replaced: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.type_text(text, window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _replaced: Option<Range<usize>>,
        text: &str,
        _selected: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.keys_target(window, cx);
        self.preedit = target.and_then(|target| Preedit::marked(target.pane_id(), text));
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range: Range<usize>,
        element: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(self.caret_area(window, cx).unwrap_or(element))
    }

    fn character_index_for_point(
        &mut self,
        _position: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        // The panes have no text for the input method to point into.
        None
    }
}

/// An invisible element that hands the platform this handler while `focus` has the keyboard, so
/// that a window with a terminal or an editor in front takes text as the composer does.
pub(crate) fn input_surface(focus: &FocusHandle, cx: &mut Context<Workbench>) -> impl IntoElement {
    let (focus, workbench) = (focus.clone(), cx.entity());
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, cx| {
            let handler = ElementInputHandler::new(bounds, workbench.clone());
            window.handle_input(&focus, handler, cx);
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_composition_is_the_text_marked_and_the_pane_it_is_in() {
        let marked = Preedit::marked("pane", "\u{b4}").unwrap();
        assert_eq!(
            (marked.pane_id.as_str(), marked.text.as_str()),
            ("pane", "\u{b4}")
        );
        // An input method that marks nothing ends the composition.
        assert_eq!(Preedit::marked("pane", ""), None);
    }

    #[test]
    fn the_marked_range_counts_utf16_units() {
        assert_eq!(Preedit::marked("p", "\u{e3}").unwrap().units(), 0..1);
        assert_eq!(Preedit::marked("p", "ni\u{1f600}").unwrap().units(), 0..4);
    }

    #[test]
    fn the_pane_of_a_target_is_the_pane_that_takes_the_text() {
        assert_eq!(KeyTarget::Terminal("t".into()).pane_id(), "t");
        assert_eq!(KeyTarget::Editor("e".into()).pane_id(), "e");
    }
}
