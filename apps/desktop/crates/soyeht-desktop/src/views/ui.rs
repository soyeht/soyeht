//! Small pieces every view uses.

use crate::{
    assets::Icon,
    neo,
    theme::{
        Palette, desktop,
        elevation::{self, Elevation},
        fonts, layout, pane_interior, radius, space, type_scale,
    },
};
use gpui::{
    AnyElement, AnyView, App, AppContext, Context, Div, ElementId, FontWeight, Hsla,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement, Styled, Svg, TextRun, Window, div, font,
    linear_color_stop, linear_gradient, prelude::FluentBuilder, px, relative, rgb, svg,
};

/// The two faces besides the interface's own, each with the line height its font has. GPUI's
/// default line is 1.618 times the size, loose beside the Mac, where SwiftUI sets a line as
/// tall as its font says (`desktop.lineHeight*`); the window sets the interface face's height
/// once at its root, and these set a mono or chrome element's.
pub trait Typeface: Styled + Sized {
    /// JetBrains Mono: code, terminal output, tool rows.
    fn mono(self) -> Self {
        self.font_family(fonts::MONO)
            .line_height(relative(desktop::LINE_HEIGHT_MONO))
    }

    /// Nunito: the window's chrome (tabs, menus, usage percentages).
    fn chrome(self) -> Self {
        self.font_family(fonts::CHROME)
            .line_height(relative(desktop::LINE_HEIGHT_CHROME))
    }
}

impl<T: Styled> Typeface for T {}

/// A raised button of the top bar, `width` wide and as tall as the bar's controls, its content
/// centered. The caller says what a click does.
pub fn top_bar_button(palette: &Palette, radius: f32, elevation: Elevation, width: f32) -> Div {
    neo::raised(palette, radius, elevation)
        .w(px(width))
        .h(px(layout::TOP_BAR_CONTROL_HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
}

/// A block of window text: Nunito at `weight` and `size` points.
pub fn chrome_text(weight: FontWeight, size: f32) -> Div {
    div().chrome().font_weight(weight).text_size(px(size))
}

/// A line icon of `size` points in a palette color.
pub fn icon(icon: Icon, size: f32, color: u32) -> Svg {
    svg()
        .path(icon.path())
        .size(px(size))
        .flex_none()
        .text_color(rgb(color))
}

/// The chevron of a disclosure: down while what it folds is shown, right while it is folded.
pub fn disclosure_chevron(expanded: bool, color: u32) -> Svg {
    icon(
        if expanded {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        },
        desktop::DISCLOSURE_GLYPH,
        color,
    )
}

/// A provider logo of `size` points in a palette color.
pub fn logo(path: &'static str, size: f32, color: u32) -> Svg {
    svg()
        .path(path)
        .size(px(size))
        .flex_none()
        .text_color(rgb(color))
}

/// How wide `text` is in the window's chrome face (Nunito) at `size` points and `weight`.
pub fn text_width(window: &Window, text: &str, size: f32, weight: FontWeight) -> f32 {
    let mut face = font(fonts::CHROME);
    face.weight = weight;
    let run = TextRun {
        len: text.len(),
        font: face,
        color: Hsla::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(text.to_owned().into(), px(size), &[run], None);
    f32::from(line.width)
}

/// What a tooltip says, in a small raised chip of the palette.
struct Hint {
    text: SharedString,
    palette: &'static Palette,
}

impl Render for Hint {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        neo::raised(self.palette, radius::COMPACT, elevation::CHIP)
            .max_w(px(desktop::TOOLTIP_MAX_WIDTH))
            .px(px(space::X8))
            .py(px(space::X4))
            .text_size(px(type_scale::CONTROL))
            .text_color(rgb(self.palette.ink))
            .child(self.text.clone())
    }
}

/// The tooltip of an element, as the Mac's `.help` gives its controls: `.tooltip(hint(..))`.
pub fn hint(
    text: impl Into<SharedString>,
    palette: &'static Palette,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    move |_, cx| {
        cx.new(|_| Hint {
            text: text.clone(),
            palette,
        })
        .into()
    }
}

/// How many requests wait, in the warning color: the badge of a tab and of the rail's agents.
pub fn waiting_badge(count: usize, palette: &Palette) -> Div {
    div()
        .flex_none()
        .px(px(space::X6))
        .py(px(space::X2))
        .rounded_full()
        .bg(rgb(palette.warning()))
        .chrome()
        .font_weight(FontWeight::BOLD)
        .text_size(px(type_scale::MICRO))
        .text_color(rgb(palette.ink))
        .child(count.to_string())
}

/// The dot that says an agent is working: the accent, seven points wide.
pub fn working_dot(palette: &Palette) -> Div {
    div()
        .flex_none()
        .size(px(space::X7))
        .rounded_full()
        .bg(rgb(palette.accent))
}

/// A line of small mono type in the muted ink that is cut short when it does not fit: a status,
/// a branch.
pub fn mono_note(palette: &Palette) -> Div {
    div()
        .mono()
        .truncate()
        .font_weight(FontWeight::MEDIUM)
        .text_size(px(type_scale::SMALL))
        .text_color(rgb(palette.muted))
}

/// A raised pill that opens or closes the choices of an option: an icon, the label and a chevron
/// that turns over while the choices are open. What a click does is the caller's to attach.
pub fn option_pill(
    palette: &'static Palette,
    key: impl Into<ElementId>,
    glyph: Icon,
    text: impl Into<SharedString>,
    open: bool,
) -> gpui::Stateful<Div> {
    let chevron = if open {
        Icon::ChevronUp
    } else {
        Icon::ChevronDown
    };
    raised_chip(
        palette,
        glyph,
        desktop::INLINE_GLYPH,
        text.into(),
        Some(chevron),
    )
    .id(key)
    .gap(px(space::X6))
    .px(px(space::X10))
    .py(px(space::X6))
    .cursor_pointer()
    .own_press()
}

/// The raised capsule every chip and pill of the windows shares: an icon of `glyph_size`, the
/// label and, when `chevron` names one, a muted chevron. The caller sets the spacing and what a
/// click does.
pub fn raised_chip(
    palette: &Palette,
    glyph: Icon,
    glyph_size: f32,
    label: impl IntoElement,
    chevron: Option<Icon>,
) -> Div {
    neo::raised(palette, radius::PILL, elevation::CHIP)
        .flex()
        .flex_row()
        .items_center()
        .chrome()
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(type_scale::CONTROL))
        .text_color(rgb(palette.secondary))
        .child(icon(glyph, glyph_size, palette.secondary))
        .child(label)
        .children(chevron.map(|chevron| icon(chevron, desktop::CHEVRON_GLYPH, palette.muted)))
}

/// The on/off switch: an accent capsule with the knob on the right when on, a well with the
/// knob on the left when off.
pub fn switch(on: bool, palette: &Palette) -> Div {
    row()
        .w(px(desktop::LOOP_TOGGLE_WIDTH))
        .h(px(desktop::LOOP_TOGGLE_HEIGHT))
        .p(px(space::X2))
        .rounded_full()
        .bg(rgb(if on { palette.accent } else { palette.well }))
        .when(on, |track| track.justify_end())
        .child(
            div()
                .size(px(desktop::LOOP_TOGGLE_KNOB))
                .rounded_full()
                .bg(rgb(palette.on_accent)),
        )
}

/// A stable element id from a kind and the thing it belongs to.
pub fn id(kind: &str, owner: &str) -> SharedString {
    SharedString::from(format!("{kind}-{owner}"))
}

/// A row whose children are centered on the cross axis.
pub fn row() -> Div {
    div().flex().flex_row().items_center()
}

/// Text in a mono well: command output and code, as the Mac shows them.
pub fn code_well(text: impl Into<SharedString>, palette: &Palette) -> Div {
    div()
        .p(px(pane_interior::ROW_SPACING))
        .rounded(px(pane_interior::WELL_RADIUS))
        .bg(rgb(palette.well))
        .mono()
        .text_size(px(pane_interior::MONO_POINT_SIZE))
        .child(text.into())
}

/// A column.
pub fn column() -> Div {
    div().flex().flex_col()
}

/// An element that fills the room it is given and shows nothing, to push its siblings apart.
pub fn spacer() -> Div {
    div().flex_1().min_w_0()
}

/// A thin horizontal rule in `color`.
pub fn rule(color: u32, thickness: f32) -> AnyElement {
    div()
        .w_full()
        .h(px(thickness))
        .flex_none()
        .bg(rgb(color))
        .into_any_element()
}

/// A press on an element that is its own: it does not reach what is behind the element (the
/// pane's focus handler, the window drag), so the element's click is all the press does.
pub trait OwnPress: InteractiveElement + Sized {
    fn own_press(self) -> Self {
        self.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }
}

impl<T: InteractiveElement> OwnPress for T {}

/// How far a sideways scroller must be from an end before that end shows its fade.
const FADE_THRESHOLD: f32 = 0.5;

/// `content` in a scroller that moves only sideways, with a fade of the canvas color over each
/// end that hides more, so that a clipped control reads as scrollable. The wheel moves it only
/// sideways: a vertical turn belongs to whatever vertical scroller is under the pointer, a
/// transcript or a terminal, and must not also pan this one. `handle` is the scroller's own
/// scroll state; a scroller that has not been laid out yet shows no fade. The result fills the
/// room it is given, and the caller may size it further.
pub fn sideways_scroller(
    id: impl Into<ElementId>,
    handle: &ScrollHandle,
    content: impl IntoElement,
    palette: &Palette,
) -> Div {
    let mut scroller = div()
        .id(id)
        .flex()
        .size_full()
        .overflow_x_scroll()
        .track_scroll(handle)
        .child(content);
    scroller.style().restrict_scroll_to_axis = Some(true);
    div()
        .relative()
        .flex()
        .flex_1()
        .min_w_0()
        .child(scroller)
        .children(edge_fades(handle, palette))
}

/// Whether a sideways scroller hides more at its left end and at its right end. A scroller that
/// has not been laid out yet hides nothing.
pub fn hidden_ends(handle: &ScrollHandle) -> (bool, bool) {
    let offset = f32::from(handle.offset().x);
    let hidden_right = f32::from(handle.max_offset().x) + offset;
    (offset < -FADE_THRESHOLD, hidden_right > FADE_THRESHOLD)
}

/// A fade of the canvas color over each end of a sideways scroller that hides more, so that a
/// clipped control reads as scrollable. Place the result inside the `relative` parent of the
/// scroller, after it. The handle is the scroller's own `track_scroll` handle.
pub fn edge_fades(handle: &ScrollHandle, palette: &Palette) -> Vec<AnyElement> {
    let (left, right) = hidden_ends(handle);
    let solid: Hsla = rgb(palette.canvas).into();
    let clear = Hsla { a: 0.0, ..solid };
    let fade = |from: Hsla, to: Hsla| {
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(desktop::FADE_WIDTH))
            .bg(linear_gradient(
                90.0,
                linear_color_stop(from, 0.0),
                linear_color_stop(to, 1.0),
            ))
    };
    let mut fades = Vec::new();
    if left {
        fades.push(fade(solid, clear).left_0().into_any_element());
    }
    if right {
        fades.push(fade(clear, solid).right_0().into_any_element());
    }
    fades
}
