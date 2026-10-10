//! The message line of an agent composer: a GPUI view that holds the draft, takes text from the
//! platform's text input (dead keys, input methods, emoji, composition) through an input
//! handler, and handles the keys that edit or send it.

use crate::{
    draft::Draft,
    theme::{Palette, pane_interior, space},
};
use gpui::{
    AppContext, Bounds, Context, Div, ElementInputHandler, Entity, EntityInputHandler,
    EventEmitter, FocusHandle, Focusable, HighlightStyle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Pixels, Render, SharedString, Styled, StyledText, UTF16Selection,
    Window, canvas, div, point, prelude::FluentBuilder, px, rgb, size,
};
use std::ops::Range;

/// What the field tells the workbench that owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerEvent {
    /// Return was pressed: send the message.
    Submit,
    /// Escape was pressed: close what is open.
    Dismiss,
}

/// A key that edits the draft rather than typing into it. Text itself arrives through the input
/// handler, so a key that types is not one of these.
#[derive(Debug, PartialEq, Eq)]
enum Edit {
    Newline,
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    Submit,
    Dismiss,
}

fn edit_for(key: &str, control: bool, shift: bool) -> Option<Edit> {
    match key {
        "enter" if shift => Some(Edit::Newline),
        "enter" => Some(Edit::Submit),
        "backspace" => Some(Edit::Backspace),
        "delete" => Some(Edit::Delete),
        "left" => Some(Edit::Left),
        "right" => Some(Edit::Right),
        "home" => Some(Edit::Home),
        "end" => Some(Edit::End),
        "escape" => Some(Edit::Dismiss),
        "a" if control => Some(Edit::Home),
        "e" if control => Some(Edit::End),
        _ => None,
    }
}

pub struct ComposerField {
    draft: Draft,
    focus: FocusHandle,
    palette: &'static Palette,
    placeholder: SharedString,
    /// Where the line was last painted, for the input method's candidate window.
    bounds: Option<Bounds<Pixels>>,
}

impl EventEmitter<ComposerEvent> for ComposerField {}

impl Focusable for ComposerField {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ComposerField {
    pub fn new(palette: &'static Palette, cx: &mut Context<Self>) -> Self {
        Self {
            draft: Draft::default(),
            focus: cx.focus_handle(),
            palette,
            placeholder: SharedString::default(),
            bounds: None,
        }
    }

    pub fn draft(&self) -> &Draft {
        &self.draft
    }

    /// The colors and the hint the next frame draws with.
    pub fn style(&mut self, palette: &'static Palette, placeholder: &str) {
        self.palette = palette;
        if self.placeholder != placeholder {
            self.placeholder = placeholder.to_owned().into();
        }
    }

    pub fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        self.draft.insert(text);
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.draft.clear();
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = &keystroke.modifiers;
        // The platform key is for application shortcuts; an input method owns the keys while it
        // composes.
        if modifiers.platform || self.draft.is_composing() {
            return;
        }
        let Some(edit) = edit_for(
            &keystroke.key,
            modifiers.control && !modifiers.alt,
            modifiers.shift,
        ) else {
            return;
        };
        match edit {
            Edit::Submit => cx.emit(ComposerEvent::Submit),
            Edit::Dismiss => cx.emit(ComposerEvent::Dismiss),
            Edit::Newline => self.draft.insert("\n"),
            Edit::Backspace => self.draft.backspace(),
            Edit::Delete => self.draft.delete(),
            Edit::Left => self.draft.left(),
            Edit::Right => self.draft.right(),
            Edit::Home => self.draft.home(),
            Edit::End => self.draft.end(),
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for ComposerField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let entity = cx.entity();
        let focus = self.focus.clone();
        div()
            .relative()
            .track_focus(&self.focus)
            .key_context("ComposerField")
            .on_key_down(cx.listener(Self::key_down))
            .child(line(&self.draft, &self.placeholder, focused, self.palette))
            .child(
                canvas(
                    {
                        let entity = entity.clone();
                        move |bounds, _, cx| {
                            entity.update(cx, |field, _| field.bounds = Some(bounds));
                            bounds
                        }
                    },
                    move |bounds, _, window, cx| {
                        window.handle_input(
                            &focus,
                            ElementInputHandler::new(bounds, entity.clone()),
                            cx,
                        );
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
    }
}

/// The message line: the placeholder while it is empty, else the text, with a block caret while
/// the composer has the keyboard and the composing text underlined.
fn line(draft: &Draft, placeholder: &SharedString, focused: bool, palette: &Palette) -> Div {
    let block = HighlightStyle {
        background_color: Some(rgb(palette.ink).into()),
        color: Some(rgb(palette.face).into()),
        ..HighlightStyle::default()
    };
    let text = draft.text();
    if text.is_empty() {
        return div()
            .flex()
            .flex_row()
            .gap(px(space::X2))
            .when(focused, |row| {
                row.child(StyledText::new(" ").with_highlights([(0..1, block)]))
            })
            .child(
                div()
                    .text_color(rgb(palette.secondary))
                    .child(placeholder.clone()),
            );
    }
    let caret = draft.caret();
    let mut shown = text.to_owned();
    let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    if let Some(marked) = draft.marked_bytes() {
        highlights.push((
            marked,
            HighlightStyle {
                underline: Some(gpui::UnderlineStyle {
                    thickness: px(pane_interior::SEPARATOR_WIDTH),
                    color: Some(rgb(palette.ink).into()),
                    wavy: false,
                }),
                ..HighlightStyle::default()
            },
        ));
    }
    if focused && highlights.is_empty() {
        match text[caret..].chars().next() {
            Some(next) => highlights.push((caret..caret + next.len_utf8(), block)),
            None => {
                shown.push(' ');
                highlights.push((caret..caret + 1, block));
            }
        }
    }
    div()
        .text_color(rgb(palette.ink))
        .child(StyledText::new(shown).with_highlights(highlights))
}

impl EntityInputHandler for ComposerField {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let bytes = self.draft.bytes_of(&range);
        adjusted.replace(self.draft.units_of(&bytes));
        Some(self.draft.text()[bytes].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let caret = self.draft.caret_utf16();
        Some(UTF16Selection {
            range: caret..caret,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.draft.marked_utf16()
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.draft.unmark();
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.draft.replace_in_range(range, text);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.draft.replace_and_mark(range, text, selected);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        element: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // Close enough for the candidate window: the line the text is on.
        let anchor = self.bounds.unwrap_or(element);
        Some(Bounds::new(
            point(anchor.left(), anchor.top()),
            size(px(1.0), anchor.size.height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

/// A new field entity.
pub fn new_field(palette: &'static Palette, cx: &mut gpui::App) -> Entity<ComposerField> {
    cx.new(|cx| ComposerField::new(palette, cx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_sends_and_shift_enter_adds_a_line() {
        assert_eq!(edit_for("enter", false, false), Some(Edit::Submit));
        assert_eq!(edit_for("enter", false, true), Some(Edit::Newline));
    }

    #[test]
    fn editing_keys_are_recognised_and_typing_keys_are_left_to_the_platform() {
        assert_eq!(edit_for("backspace", false, false), Some(Edit::Backspace));
        assert_eq!(edit_for("a", true, false), Some(Edit::Home));
        assert_eq!(edit_for("a", false, false), None);
        assert_eq!(edit_for("c", true, false), None);
        assert_eq!(edit_for("f1", false, false), None);
    }
}
