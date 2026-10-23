//! A presentation pane, as the Mac draws it: a rail of the slides (a thumbnail for each, or a strip
//! of numbers while the pane is narrow), the Markdown source in a 16:9 stage, and under them the
//! line that says what the source means. The Present button of the header starts a slide show
//! over the pane. The layout is `soyeht_client::writing::presentation`, whose cases the Mac
//! passes too; this draws it. The source is the text pane's field, and what is typed reaches the
//! engine as `writing_flow` says.

use super::{
    prelude::*,
    ui::{chrome_text, mono_label, pressable},
};
use crate::{
    composer_field::ComposerField,
    theme::{Palette, desktop, fonts, pane_interior, radius, space, type_scale},
    workbench::Workbench,
};
use gpui::{
    App, Bounds, Entity, FocusHandle, KeyDownEvent, Keystroke, MouseButton, Pixels, ScrollHandle,
    canvas,
};
use soyeht_client::writing::document::position_of;
use soyeht_client::writing::presentation::{
    self, Layout, Metrics, Navigation, Rect, Show, ShowInput,
};
use soyeht_protocol::WritingSlide;

/// The measures of the layout, from the tokens.
fn metrics() -> Metrics {
    Metrics {
        rail_width: desktop::PRESENTATION_RAIL_WIDTH,
        number_strip_height: desktop::PRESENTATION_NUMBER_STRIP_HEIGHT,
        footer_height: desktop::PRESENTATION_FOOTER_HEIGHT,
        inset: desktop::PRESENTATION_INSET,
        rail_divider: desktop::PRESENTATION_RAIL_DIVIDER,
        minimum_stage: (
            desktop::PRESENTATION_MINIMUM_STAGE_WIDTH,
            desktop::PRESENTATION_MINIMUM_STAGE_HEIGHT,
        ),
        maximum_stage: (
            desktop::PRESENTATION_MAXIMUM_STAGE_WIDTH,
            desktop::PRESENTATION_MAXIMUM_STAGE_HEIGHT,
        ),
    }
}

/// Where a rectangle of the layout is, in the body.
fn placed(rect: Rect) -> Div {
    div()
        .absolute()
        .left(px(rect.x))
        .top(px(rect.y))
        .w(px(rect.width))
        .h(px(rect.height))
}

/// What a keystroke means to a slide show, and whether the platform's command key is held.
fn show_input(keystroke: &Keystroke) -> (ShowInput, bool) {
    let input = match keystroke.key.as_str() {
        "right" => ShowInput::RightArrow,
        "left" => ShowInput::LeftArrow,
        "pagedown" => ShowInput::PageDown,
        "pageup" => ShowInput::PageUp,
        "space" => ShowInput::Space,
        "escape" => ShowInput::Escape,
        _ => ShowInput::Other,
    };
    let modifiers = &keystroke.modifiers;
    (input, modifiers.platform || modifiers.control)
}

impl Workbench {
    /// Remembers the size the body of a presentation pane was drawn at, which its layout is made
    /// from; the pane is drawn again when it changed.
    fn record_body(&mut self, pane_id: &str, bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        let drawn = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let ui = &mut self.panes.of(pane_id).writing;
        let moved = |known: (f32, f32)| {
            (known.0 - drawn.0).abs() >= 0.5 || (known.1 - drawn.1).abs() >= 0.5
        };
        if ui.body.is_none_or(moved) {
            ui.body = Some(drawn);
            cx.notify();
        }
    }

    /// The body of a presentation pane: the slides, the source and its line, and the slide show
    /// while one is on.
    pub(super) fn presentation_content(
        &mut self,
        (palette, pane_id): (&'static Palette, &str),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_writing_field(pane_id, cx);
        let slides = self.slides_of(pane_id);
        // The show ends with its slides, and when the keyboard goes to another pane (an agent
        // opened beside the presentation), which would leave it with no way to be closed.
        let away = self
            .state
            .focused_pane()
            .is_none_or(|focused| focused.pane_id != pane_id);
        let show = &mut self.panes.of(pane_id).writing.show;
        let emptied = show
            .as_mut()
            .is_some_and(|show| show.reconcile(slides.len()));
        if away && show.take().is_some() {
            cx.notify();
        } else if emptied {
            self.end_show(pane_id, cx);
        }
        let field = self.writing_field(cx, pane_id);
        field.update(cx, |field, _| field.style(palette, "Start writing"));
        let focus = self.show_focus(pane_id, cx);
        // The slide the caret is on, which the engine's slides (they follow the typing a moment
        // later) place it in; a caret between two slides leaves the one before.
        let (text, caret) = {
            let field = field.read(cx);
            (field.draft().text().to_owned(), field.selection_ends().1)
        };
        let line = position_of(&text, 0, caret).line;
        let ui = &mut self.panes.of(pane_id).writing;
        if let Some(index) = presentation::slide_at_line(&slides, line) {
            ui.slide = index;
        }
        let (body, show, scrolls) = (ui.body, ui.show, (ui.scroll.clone(), ui.rail.clone()));
        let look = Look {
            palette,
            pane_id,
            selected: ui.slide,
        };
        let owner = pane_id.to_owned();
        let workbench = cx.entity();
        let measure = canvas(
            move |bounds, _, cx: &mut App| {
                workbench.update(cx, |workbench, cx| {
                    workbench.record_body(&owner, bounds, cx)
                });
            },
            |_, (), _, _| {},
        )
        .absolute()
        .size_full();
        let mut stack = div()
            .id(id("presentation", pane_id))
            .relative()
            .flex_1()
            .min_h_0()
            .bg(rgb(palette.face))
            .child(measure);
        if let Some(body) = body {
            let layout = presentation::layout(body, &metrics());
            stack = stack
                .child(look.navigation(&layout, &slides, &scrolls.1, cx))
                .child(look.source(layout.stage, field, &scrolls.0))
                .child(look.footer(layout.footer));
            if let Some(show) = show {
                stack = stack.child(look.show(show, &slides, body, &focus, cx));
            }
        }
        stack.into_any_element()
    }
}

/// What the parts of a presentation pane are drawn with.
struct Look<'a> {
    palette: &'static Palette,
    pane_id: &'a str,
    /// The slide the caret is on.
    selected: u32,
}

/// Small type in the muted ink: what a slide says under its title, and the line under the source.
fn note(palette: &Palette, text: &str) -> Div {
    chrome_text(FontWeight::NORMAL, type_scale::MICRO)
        .child(SharedString::from(text.to_owned()))
        .text_color(rgb(palette.muted))
}

impl Look<'_> {
    /// The rail of thumbnails, or the strip of numbers while the pane is narrow.
    fn navigation(
        &self,
        layout: &Layout,
        slides: &[WritingSlide],
        scroll: &ScrollHandle,
        cx: &mut Context<Workbench>,
    ) -> AnyElement {
        let palette = self.palette;
        if layout.navigation == Navigation::Numbers {
            let numbers = slides.iter().map(|slide| self.number(slide.index, cx));
            let strip = row().h_full().gap(px(space::X6)).px(px(space::X10));
            return placed(layout.rail)
                .id(id("slide-numbers", self.pane_id))
                .overflow_x_scroll()
                .child(strip.children(numbers))
                .into_any_element();
        }
        let thumbnails: Vec<AnyElement> = slides
            .iter()
            .map(|slide| self.thumbnail(slide, cx))
            .collect();
        let rail = layout.rail;
        let divider = Rect {
            x: rail.width,
            width: desktop::PRESENTATION_RAIL_DIVIDER,
            ..rail
        };
        let title = chrome_text(FontWeight::BOLD, type_scale::MICRO)
            .text_color(rgb(palette.secondary))
            .child("SLIDES");
        let count = note(palette, &slides.len().to_string()).mono();
        let list = column()
            .id(id("slide-rail", self.pane_id))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(scroll)
            .gap(px(space::X10))
            .p(px(space::X4))
            .children(thumbnails);
        let shelf = placed(rail).flex().flex_col().gap(px(space::X10));
        div()
            .child(
                shelf
                    .p(px(space::X12))
                    .child(row().child(title).child(spacer()).child(count))
                    .child(list),
            )
            .child(placed(divider).bg(rgb(palette.well)))
            .into_any_element()
    }

    /// A slide's number as a button of the strip.
    fn number(&self, index: u32, cx: &mut Context<Workbench>) -> AnyElement {
        let owner = self.pane_id.to_owned();
        let disc = self
            .disc(index)
            .id(id(&format!("slide-number-{index}"), self.pane_id));
        pressable(disc, true, cx, move |this, cx| {
            this.select_slide(&owner, index, cx)
        })
        .cursor_pointer()
        .into_any_element()
    }

    /// The numbered disc of a slide, in a thumbnail and in the strip.
    fn disc(&self, index: u32) -> Div {
        let palette = self.palette;
        let chosen = index == self.selected;
        let (fill, ink) = if chosen {
            (palette.accent, palette.on_accent)
        } else {
            (palette.well, palette.secondary)
        };
        mono_label()
            .text_size(px(type_scale::MICRO))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .size(px(desktop::PRESENTATION_NUMBER_DISC))
            .rounded_full()
            .bg(rgb(fill))
            .text_color(rgb(ink))
            .child((index + 1).to_string())
    }

    /// A slide in the rail: its number, its title and its notes.
    fn thumbnail(&self, slide: &WritingSlide, cx: &mut Context<Workbench>) -> AnyElement {
        let palette = self.palette;
        let index = slide.index;
        let (edge, width) = if index == self.selected {
            (palette.accent, desktop::PRESENTATION_SELECTED_BORDER)
        } else {
            (palette.well, desktop::HAIRLINE)
        };
        let owner = self.pane_id.to_owned();
        let top = row().items_start().child(self.disc(index)).child(spacer());
        let images = (!slide.image_paths.is_empty())
            .then(|| icon(Icon::Image, desktop::INLINE_GLYPH, palette.secondary));
        let title = chrome_text(FontWeight::BOLD, type_scale::CAPTION)
            .text_color(rgb(palette.ink))
            .line_clamp(2)
            .child(SharedString::from(
                presentation::slide_title(&slide.heading).to_owned(),
            ));
        let card = column()
            .id(id(&format!("slide-{index}"), self.pane_id))
            .w_full()
            .min_h(px(desktop::PRESENTATION_THUMBNAIL_HEIGHT))
            .flex_none()
            .p(px(space::X10))
            .gap(px(space::X6))
            .rounded(px(radius::CONTROL))
            .bg(rgb(palette.face))
            .border(px(width))
            .border_color(rgb(edge))
            .cursor_pointer()
            .child(top.children(images))
            .child(title)
            .child(note(palette, presentation::slide_notes(&slide.notes)).line_clamp(2));
        pressable(card, true, cx, move |this, cx| {
            this.select_slide(&owner, index, cx)
        })
        .into_any_element()
    }

    /// The source in its 16:9 stage.
    fn source(
        &self,
        stage: Rect,
        field: Entity<ComposerField>,
        scroll: &ScrollHandle,
    ) -> AnyElement {
        let palette = self.palette;
        let text = div()
            .id(id("presentation-source", self.pane_id))
            .size_full()
            .overflow_y_scroll()
            .track_scroll(scroll)
            .px(px(desktop::PRESENTATION_SOURCE_INSET_X))
            .py(px(desktop::PRESENTATION_SOURCE_INSET_Y))
            .font_family(fonts::MONO)
            .text_size(px(pane_interior::MONO_POINT_SIZE))
            .line_height(px(desktop::PRESENTATION_LINE_HEIGHT))
            .text_color(rgb(palette.ink))
            .cursor_text()
            .child(field);
        placed(stage)
            .rounded(px(radius::CONTROL))
            .border(px(desktop::HAIRLINE))
            .border_color(rgb(palette.well))
            .bg(rgb(palette.face))
            .overflow_hidden()
            .child(text)
            .into_any_element()
    }

    /// The line under the source: what its parts mean, and what it is written in.
    fn footer(&self, at: Rect) -> AnyElement {
        let palette = self.palette;
        placed(at)
            .flex()
            .items_center()
            .gap(px(space::X8))
            .px(px(space::X12))
            .child(icon(Icon::Info, desktop::INLINE_GLYPH, palette.muted))
            .child(
                note(palette, presentation::HINT)
                    .flex_1()
                    .min_w_0()
                    .truncate(),
            )
            .child(note(palette, presentation::FORMAT).flex_none())
            .into_any_element()
    }

    /// The slide show: the current slide on the letterbox, over the whole body. A key or a click
    /// moves it (`presentation::show_command`).
    fn show(
        &self,
        show: Show,
        slides: &[WritingSlide],
        body: (f32, f32),
        focus: &FocusHandle,
        cx: &mut Context<Workbench>,
    ) -> AnyElement {
        let palette = self.palette;
        let stage = presentation::show_stage(body);
        let sizes = presentation::slide_metrics((stage.width, stage.height));
        let heading = slides.get(show.index()).map_or("", |slide| &slide.heading);
        let (keys, clicks) = (self.pane_id.to_owned(), self.pane_id.to_owned());
        let title = div()
            .text_center()
            .font_weight(FontWeight::BOLD)
            .text_size(px(sizes.title_size))
            .text_color(rgb(palette.ink))
            .line_clamp(3)
            .child(SharedString::from(
                presentation::slide_title(heading).to_owned(),
            ));
        let slide = placed(stage)
            .flex()
            .items_center()
            .justify_center()
            .p(px(sizes.padding))
            .bg(rgb(palette.face))
            .overflow_hidden()
            .child(title);
        div()
            .id(id("slide-show", self.pane_id))
            .absolute()
            .size_full()
            .track_focus(focus)
            .bg(rgb(palette.presentation_letterbox))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                this.show_input(&keys, show_input(&event.keystroke), cx);
                cx.stop_propagation();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.show_input(&clicks, (ShowInput::Click, false), cx);
                }),
            )
            .child(slide)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tokens_are_the_measures_the_vector_holds_for_the_mac() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../contracts/vectors/presentation.json");
        let text = std::fs::read_to_string(path).unwrap();
        let vector: serde_json::Value = serde_json::from_str(&text).unwrap();
        let held = &vector["layout"]["metrics"];
        let number = |key: &str| held[key].as_f64().unwrap() as f32;
        let pair = |key: &str| (number_at(&held[key], 0), number_at(&held[key], 1));
        fn number_at(value: &serde_json::Value, at: usize) -> f32 {
            value[at].as_f64().unwrap() as f32
        }
        let tokens = metrics();
        assert_eq!(tokens.rail_width, number("railWidth"));
        assert_eq!(tokens.number_strip_height, number("numberStripHeight"));
        assert_eq!(tokens.footer_height, number("footerHeight"));
        assert_eq!(tokens.inset, number("inset"));
        assert_eq!(tokens.rail_divider, number("railDivider"));
        assert_eq!(tokens.minimum_stage, pair("minimumStage"));
        assert_eq!(tokens.maximum_stage, pair("maximumStage"));
    }

    #[test]
    fn the_keys_of_a_slide_show_are_the_ones_the_mac_reads() {
        let input = |keys: &str| show_input(&Keystroke::parse(keys).unwrap());
        assert_eq!(input("right"), (ShowInput::RightArrow, false));
        assert_eq!(input("left"), (ShowInput::LeftArrow, false));
        assert_eq!(input("pagedown"), (ShowInput::PageDown, false));
        assert_eq!(input("pageup"), (ShowInput::PageUp, false));
        assert_eq!(input("space"), (ShowInput::Space, false));
        assert_eq!(input("escape"), (ShowInput::Escape, false));
        assert_eq!(input("a"), (ShowInput::Other, false));
        assert_eq!(input("ctrl-right"), (ShowInput::RightArrow, true));
    }
}
