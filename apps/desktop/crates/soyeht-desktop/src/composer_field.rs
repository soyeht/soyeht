//! The message line of an agent composer: a GPUI view that holds the draft (a `TextInput`, which
//! owns every editing rule), takes text from the platform's text input (dead keys, input
//! methods, emoji, composition) through an input handler, and forwards the keys, the pointer and
//! the clipboard to it. With `ComposerField::prose` it is the text of a writing pane instead
//! (`prose`).

mod page;
mod prose;

use page::Laid;
use prose::Prose;
pub use prose::{Looks, ProseEvent};

use crate::{
    pointer::follow,
    shortcuts,
    theme::{Palette, pane_interior, space},
    views::selectable::clear_selection,
};
use gpui::{
    App, AppContext, Bounds, ClipboardEntry, ClipboardItem, Context, Div, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, HighlightStyle,
    InteractiveElement, IntoElement, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement, Pixels, Render, SharedString, Styled, StyledText, TextLayout,
    UTF16Selection, Window, canvas, combine_highlights, div, fill, point, prelude::FluentBuilder,
    px, rgb, size,
};
use soyeht_client::{
    editor::{keys::Chord, selection::Motion},
    text_input::{
        TextInput,
        keys::{Completion, Edit, completion, edit},
    },
};
use std::ops::Range;

/// What the field tells the workbench that owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerEvent {
    /// Return was pressed: send the message.
    Submit,
    /// Escape was pressed: close what is open.
    Dismiss,
    /// A paste carries files or a picture, which the owner may attach; the field pastes text only.
    Attach,
}

pub struct ComposerField {
    draft: TextInput,
    focus: FocusHandle,
    palette: &'static Palette,
    placeholder: SharedString,
    /// Where the line was last painted, for the input method's candidate window.
    bounds: Option<Bounds<Pixels>>,
    /// The text as it was last laid out, to turn a pointer position into an offset and back.
    /// Only a layout that has been painted is kept: asking an unpainted one panics.
    layout: Option<Laid>,
    /// The horizontal position the caret keeps while it moves up and down wrapped lines.
    goal_x: Option<Pixels>,
    /// The pointer button is down and extends the selection.
    dragging: bool,
    /// A list of completions is open over the line: it takes the arrows, Return and Tab.
    completing: bool,
    prose: Option<Prose>,
}

impl EventEmitter<ComposerEvent> for ComposerField {}
/// What an open list of completions is asked, by the keys it takes from the line.
impl EventEmitter<Completion> for ComposerField {}

impl Focusable for ComposerField {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ComposerField {
    pub fn new(palette: &'static Palette, cx: &mut Context<Self>) -> Self {
        Self {
            draft: TextInput::default(),
            focus: cx.focus_handle(),
            palette,
            placeholder: SharedString::default(),
            bounds: None,
            layout: None,
            goal_x: None,
            dragging: false,
            completing: false,
            prose: None,
        }
    }

    pub fn draft(&self) -> &TextInput {
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
        self.goal_x = None;
        cx.notify();
    }

    /// Whether a list of completions is open over the line, which then takes its keys.
    pub fn complete_with_list(&mut self, open: bool) {
        self.completing = open;
    }

    /// Puts `text` in place of everything written, the caret after it.
    pub fn replace(&mut self, text: &str, cx: &mut Context<Self>) {
        self.draft.select_all();
        self.draft.insert(text);
        self.goal_x = None;
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.draft.clear();
        self.goal_x = None;
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        // An input method owns the keys while it composes.
        if self.draft.is_composing() {
            return;
        }
        if self.prose.is_some() && self.prose_key(&event.keystroke, cx) {
            return;
        }
        if self.completing
            && let Some(asked) = completion_of(&event.keystroke)
        {
            cx.emit(asked);
            cx.stop_propagation();
            return;
        }
        let Some(edit) = edit_of(&event.keystroke) else {
            return;
        };
        // With nothing selected here, a copy is for what is selected in the transcript. Prose
        // leaves Escape to whatever is open around it.
        if (edit == Edit::Copy && !self.draft.has_selection())
            || (edit == Edit::Dismiss && self.prose.is_some())
        {
            return;
        }
        if !matches!(
            edit,
            Edit::Move {
                motion: Motion::Up | Motion::Down,
                ..
            }
        ) {
            self.goal_x = None;
        }
        match edit {
            Edit::Submit if self.prose.is_some() => self.draft.insert("\n"),
            Edit::Submit => cx.emit(ComposerEvent::Submit),
            Edit::Dismiss => cx.emit(ComposerEvent::Dismiss),
            Edit::Newline => self.draft.insert("\n"),
            Edit::Backspace => self.draft.backspace(),
            Edit::Delete => self.draft.delete(),
            Edit::DeleteWord { backward } => self.draft.delete_word(backward),
            Edit::Move { motion, extend } => self.step(motion, extend),
            Edit::SelectAll => self.draft.select_all(),
            Edit::Copy => self.copy(cx),
            Edit::Cut => self.cut(cx),
            Edit::Paste => self.paste(cx),
            Edit::Undo => {
                self.draft.undo();
            }
            Edit::Redo => {
                self.draft.redo();
            }
        }
        cx.stop_propagation();
        self.edited(true, cx);
        cx.notify();
    }

    /// A motion of the caret. Up and down follow the lines as they are drawn, wrapped ones
    /// included, when a layout is known; otherwise the lines of the text.
    fn step(&mut self, motion: Motion, extend: bool) {
        let up = motion == Motion::Up;
        if !up && motion != Motion::Down {
            self.draft.move_by(motion, extend);
            return;
        }
        let drawn = self.layout.as_ref().and_then(|layout| {
            let here = layout.position_for_index(self.draft.caret())?;
            Some((layout, here))
        });
        let Some((layout, here)) = drawn else {
            self.draft.move_by(motion, extend);
            return;
        };
        let line = layout.line_height();
        let x = *self.goal_x.get_or_insert(here.x);
        // The middle of the line above or below.
        let y = if up {
            here.y - line * 0.5
        } else {
            here.y + line * 1.5
        };
        let (Ok(at) | Err(at)) = layout.index_for_position(point(x, y));
        self.draft.place(at, extend);
    }

    fn copy(&self, cx: &mut Context<Self>) {
        if let Some(text) = self.draft.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
        }
    }

    fn cut(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.draft.cut() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Pastes the clipboard's text; files and pictures are for the owner, who may attach them.
    fn paste(&mut self, cx: &mut Context<Self>) {
        match cx.read_from_clipboard() {
            Some(item) if carries_files(&item) => cx.emit(ComposerEvent::Attach),
            Some(item) => {
                if let Some(text) = item.text() {
                    self.draft.insert(&text);
                }
            }
            None => {}
        }
    }

    /// The offset of the text under a window position.
    fn offset_at(&self, position: gpui::Point<Pixels>) -> usize {
        let at = self
            .layout
            .as_ref()
            .map_or(self.draft.text().len(), |layout| {
                let (Ok(at) | Err(at)) = layout.index_for_position(position);
                at
            });
        at.min(self.draft.text().len())
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        clear_selection(cx);
        if self.draft.is_composing() {
            return;
        }
        let at = self.offset_at(event.position);
        self.goal_x = None;
        match event.click_count {
            1 => {
                self.draft.place(at, event.modifiers.shift);
                self.dragging = true;
            }
            2 => self.draft.select_word(at),
            _ => self.draft.select_line(at),
        }
        self.edited(false, cx);
        cx.notify();
    }

    fn drag_to(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            // The button was released where this window did not see it.
            self.dragging = false;
            return;
        }
        let at = self.offset_at(event.position);
        self.draft.place(at, true);
        self.edited(false, cx);
        cx.notify();
    }

    /// While a selection is dragged the pointer is followed over the whole window, not only over
    /// the line: past its edge the selection goes on, and the button released anywhere ends it.
    fn follow_drag(entity: &Entity<Self>, window: &mut Window) {
        let (moved, released) = (entity.clone(), entity.clone());
        follow(
            window,
            move |event, _, cx| moved.update(cx, |field, cx| field.drag_to(event, cx)),
            move |_, cx| released.update(cx, |field, _| field.dragging = false),
        );
    }
}

impl ComposerField {
    /// The field was painted: the layout is kept for the pointer and the input method, a drag
    /// is followed, and prose scrolls to the caret that moved.
    fn painted(
        entity: &Entity<Self>,
        (bounds, layout, focused): (Bounds<Pixels>, Option<Laid>, bool),
        window: &mut Window,
        cx: &mut App,
    ) {
        let (dragging, reveal, caret) = entity.update(cx, |field, _| {
            field.layout = layout;
            (
                field.dragging,
                field.take_reveal(),
                field.thin_caret(bounds, focused),
            )
        });
        if dragging {
            Self::follow_drag(entity, window);
        }
        if let Some(reveal) = reveal {
            reveal.apply(window);
        }
        if let Some((caret, color)) = caret {
            window.paint_quad(fill(caret, rgb(color)));
        }
    }

    /// Text arrived from the platform's input: the caret has no column to keep any more.
    fn typed(&mut self, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.edited(true, cx);
        cx.notify();
    }
}

impl Render for ComposerField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let entity = cx.entity();
        let focus = self.focus.clone();
        let looks = self.looks();
        let (text, layout) = match self.page_styles() {
            Some(styles) if !self.draft.text().is_empty() => {
                let (page, laid) = page::page(&self.draft, styles, self.palette);
                (page, Some(laid))
            }
            _ => {
                let (line, layout) = line(
                    &self.draft,
                    &self.placeholder,
                    focused && self.prose.is_none(),
                    self.palette,
                    &looks,
                );
                (line, layout.map(Laid::Whole))
            }
        };
        div()
            .relative()
            .track_focus(&self.focus)
            .key_context("ComposerField")
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .child(text)
            .child(
                canvas(
                    {
                        let entity = entity.clone();
                        move |bounds, _, cx: &mut App| {
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
                        ComposerField::painted(&entity, (bounds, layout, focused), window, cx);
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
    }
}

/// The message line: the placeholder while it is empty, else the text, with the selection
/// shaded or a block caret while the composer has the keyboard, and the composing text
/// underlined. The layout comes back to be kept once the line is painted.
fn line(
    draft: &TextInput,
    placeholder: &SharedString,
    block_caret: bool,
    palette: &Palette,
    looks: &[(Range<usize>, HighlightStyle)],
) -> (Div, Option<TextLayout>) {
    let block = HighlightStyle {
        background_color: Some(rgb(palette.ink).into()),
        color: Some(rgb(palette.face).into()),
        ..HighlightStyle::default()
    };
    let text = draft.text();
    if text.is_empty() {
        let empty = div()
            .flex()
            .flex_row()
            .gap(px(space::X2))
            .when(block_caret, |row| {
                row.child(StyledText::new(" ").with_highlights([(0..1, block)]))
            })
            .child(
                div()
                    .text_color(rgb(palette.secondary))
                    .child(placeholder.clone()),
            );
        return (empty, None);
    }
    let caret = draft.caret();
    let mut shown = text.to_owned();
    let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    if let Some(marked) = draft.marked_bytes() {
        highlights.push((marked, composing_style(palette)));
    }
    if highlights.is_empty() && draft.has_selection() {
        highlights.push((draft.selection(), selected_style(palette)));
    } else if block_caret && highlights.is_empty() {
        match text[caret..].chars().next() {
            Some(next) => highlights.push((caret..caret + next.len_utf8(), block)),
            None => {
                shown.push(' ');
                highlights.push((caret..caret + 1, block));
            }
        }
    }
    // The looks of prose go under the selection and the caret.
    let highlights: Vec<_> = if looks.is_empty() {
        highlights
    } else {
        combine_highlights(looks.iter().cloned(), highlights).collect()
    };
    let styled = StyledText::new(shown).with_highlights(highlights);
    let layout = styled.layout().clone();
    (
        div().text_color(rgb(palette.ink)).child(styled),
        Some(layout),
    )
}

/// How the text being composed by an input method is drawn: underlined.
fn composing_style(palette: &Palette) -> HighlightStyle {
    HighlightStyle {
        underline: Some(gpui::UnderlineStyle {
            thickness: px(pane_interior::SEPARATOR_WIDTH),
            color: Some(rgb(palette.ink).into()),
            wavy: false,
        }),
        ..HighlightStyle::default()
    }
}

/// How the selected text is drawn: on the selection color.
fn selected_style(palette: &Palette) -> HighlightStyle {
    HighlightStyle {
        background_color: Some(rgb(palette.selection).into()),
        ..HighlightStyle::default()
    }
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
        let (range, reversed) = self.draft.selection_utf16();
        Some(UTF16Selection { range, reversed })
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
        self.typed(cx);
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
        self.typed(cx);
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        element: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // The candidate window sits under the character the range starts at, else under the line.
        if let Some(layout) = &self.layout
            && let Some(at) = layout.position_for_index(self.draft.bytes_of(&range).start)
        {
            return Some(Bounds::new(at, size(px(1.0), layout.line_height())));
        }
        let anchor = self.bounds.unwrap_or(element);
        Some(Bounds::new(
            point(anchor.left(), anchor.top()),
            size(px(1.0), anchor.size.height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let (Ok(at) | Err(at)) = layout.index_for_position(position);
        let at = at.min(self.draft.text().len());
        Some(self.draft.units_of(&(at..at)).start)
    }
}

/// Whether a paste is files or a picture rather than text.
fn carries_files(item: &ClipboardItem) -> bool {
    item.entries().iter().any(|entry| {
        matches!(
            entry,
            ClipboardEntry::ExternalPaths(_) | ClipboardEntry::Image(_)
        )
    })
}

/// The edit a key press asks of a text field, if it asks one.
pub(crate) fn edit_of(keystroke: &Keystroke) -> Option<Edit> {
    edit(&chord_of(keystroke), shortcuts::ON_MAC)
}

/// What a key press asks of an open list of completions, if it asks anything.
fn completion_of(keystroke: &Keystroke) -> Option<Completion> {
    completion(&chord_of(keystroke))
}

fn chord_of(keystroke: &Keystroke) -> Chord<'_> {
    let modifiers = &keystroke.modifiers;
    Chord {
        key: &keystroke.key,
        // What a key types arrives through the input handler, not here.
        text: None,
        control: modifiers.control,
        alt: modifiers.alt,
        shift: modifiers.shift,
        platform: modifiers.platform,
    }
}

/// A new field entity.
pub fn new_field(palette: &'static Palette, cx: &mut gpui::App) -> Entity<ComposerField> {
    cx.new(|cx| ComposerField::new(palette, cx))
}
