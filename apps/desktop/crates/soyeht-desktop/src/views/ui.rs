//! Small pieces every view uses.

use crate::{
    assets::Icon,
    neo,
    theme::{
        Palette, desktop,
        elevation::{self, Elevation},
        fonts, layout, pane_interior, pane_layout, radius, space, type_scale,
    },
    workbench::Workbench,
};
use gpui::{
    AnyElement, AnyView, App, AppContext, Bounds, Context, Div, ElementId, FontWeight, Hsla,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Render, ScrollHandle,
    ScrollWheelEvent, SharedString, StatefulInteractiveElement, Styled, Svg, TextRun, Window,
    canvas, div, font, linear_color_stop, linear_gradient, prelude::FluentBuilder, px, relative,
    rgb, svg,
};

/// What the desktop client says of anything it does not serve yet, in the rail, in a pane and
/// beside a composer button: one sentence, so the three never word it differently.
pub fn not_available(what: &str) -> String {
    format!("{what} is not available in the desktop client yet.")
}

/// The well of a text field: a press anywhere in it gives the field the keyboard, whatever the
/// text inside is tall.
pub fn field_well(
    palette: &Palette,
    field: &gpui::Entity<crate::composer_field::ComposerField>,
    height: f32,
) -> Div {
    crate::composer_field::focus_on_press(
        neo::well(palette, radius::CONTROL, desktop::WELL_DEPTH)
            .h(px(height))
            .overflow_hidden(),
        field,
    )
}

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
            .relative()
            .child(super::overlay::footprint())
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

/// A listener for an element of something that has an id (a pane, a workspace): it is told the
/// id, the event and the window besides the workbench.
pub fn owned<E: 'static>(
    cx: &mut Context<Workbench>,
    owner: &str,
    act: impl Fn(&mut Workbench, &str, &E, &mut Window, &mut Context<Workbench>) + 'static,
) -> impl Fn(&E, &mut Window, &mut App) + 'static {
    let owner = owner.to_owned();
    cx.listener(move |this, event, window, cx| act(this, &owner, event, window, cx))
}

/// As [`owned`], for a handler that wants neither the event nor the window.
pub fn acting<E: 'static>(
    cx: &mut Context<Workbench>,
    owner: &str,
    act: fn(&mut Workbench, &str, &mut Context<Workbench>),
) -> impl Fn(&E, &mut Window, &mut App) + 'static {
    owned(cx, owner, move |this, owner, _: &E, _, cx| {
        act(this, owner, cx)
    })
}

/// A small button that is only an icon, with a tooltip; what a click does is the caller's.
pub fn icon_button(
    key: impl Into<ElementId>,
    glyph: Icon,
    color: u32,
    tip: impl Into<SharedString>,
    palette: &'static Palette,
) -> gpui::Stateful<Div> {
    row()
        .id(key)
        .flex_none()
        .cursor_pointer()
        .own_press()
        .tooltip(hint(tip, palette))
        .child(icon(glyph, desktop::INLINE_GLYPH, color))
}

/// A control that does `act` when pressed while `enabled`, and is dimmed while it is not.
pub fn pressable(
    control: gpui::Stateful<Div>,
    enabled: bool,
    cx: &mut Context<Workbench>,
    act: impl Fn(&mut Workbench, &mut Context<Workbench>) + 'static,
) -> gpui::Stateful<Div> {
    control
        .when(!enabled, |control| {
            control.opacity(pane_interior::DISABLED_OPACITY)
        })
        .own_press()
        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
            if enabled {
                act(this, cx);
            }
        }))
}

/// A line of caption text in the secondary ink.
pub fn caption(palette: &Palette) -> Div {
    div()
        .text_size(px(type_scale::CAPTION))
        .text_color(rgb(palette.secondary))
}

/// What stands where a place would be when it has nothing to show: its icon, large and light,
/// over a heading of the window's chrome face, and `detail` under it. The caller may add more.
pub fn empty_state(
    palette: &Palette,
    glyph: Option<Icon>,
    heading: impl Into<SharedString>,
    detail: Option<SharedString>,
) -> Div {
    column()
        .flex_1()
        .items_center()
        .justify_center()
        .gap(px(space::X18))
        .px(px(space::X18))
        .children(glyph.map(|glyph| icon(glyph, desktop::EMPTY_STATE_GLYPH, palette.secondary)))
        .child(
            chrome_text(FontWeight::BOLD, type_scale::HEADING)
                .text_color(rgb(palette.ink))
                .child(heading.into()),
        )
        .children(detail.map(|detail| caption(palette).child(detail)))
}

/// The card of a pane: the raised shell that holds its header and its body.
pub fn pane_card_shell(palette: &Palette) -> Div {
    neo::pane_shell(palette)
        .flex_1()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .p(px(pane_layout::PANE_CARD_INSET))
}

/// The bar at the top of a pane card, `height` tall, for its title and buttons.
pub fn header_bar(height: f32) -> Div {
    row()
        .h(px(height))
        .flex_none()
        .px(px(space::X12))
        .gap(px(space::X7))
}

/// A band of controls on the canvas, as the find bar of the editor is.
pub fn control_band(palette: &Palette) -> Div {
    row()
        .flex_none()
        .bg(rgb(palette.canvas))
        .px(px(space::X10))
        .py(px(space::X6))
        .gap(px(space::X6))
}

/// Small mono type in bold: a path, the name of a place, a toggle.
pub fn mono_label() -> Div {
    div()
        .mono()
        .font_weight(FontWeight::BOLD)
        .text_size(px(type_scale::SMALL))
}

/// A small capitalised heading over a list: FILES.
pub fn section_label(palette: &Palette, text: &'static str) -> Div {
    div()
        .text_color(rgb(palette.secondary))
        .text_size(px(type_scale::MICRO))
        .font_weight(FontWeight::BOLD)
        .child(text)
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
pub fn code_well(content: impl IntoElement, palette: &Palette) -> Div {
    div()
        .p(px(pane_interior::ROW_SPACING))
        .rounded(px(pane_interior::WELL_RADIUS))
        .bg(rgb(palette.well))
        .mono()
        .text_size(px(pane_interior::MONO_POINT_SIZE))
        .child(content)
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
    sideways_scroller_of(id, handle, palette, |scroller| scroller.child(content))
}

/// As [`sideways_scroller`], for a scroller whose items `fill` adds to it directly, so that the
/// handle knows where each one is (`ScrollHandle::bounds_for_item`) and can bring one into view.
pub fn sideways_scroller_of(
    id: impl Into<ElementId>,
    handle: &ScrollHandle,
    palette: &Palette,
    fill: impl FnOnce(gpui::Stateful<Div>) -> gpui::Stateful<Div>,
) -> Div {
    let mut scroller = fill(
        div()
            .id(id)
            .flex()
            .size_full()
            .overflow_x_scroll()
            .track_scroll(handle),
    );
    scroller.style().restrict_scroll_to_axis = Some(true);
    fade_frame(scroller, handle, palette)
}

/// `scroller` in a frame that fills the room it is given and draws its end fades over it.
pub fn fade_frame(scroller: gpui::Stateful<Div>, handle: &ScrollHandle, palette: &Palette) -> Div {
    div()
        .relative()
        .flex()
        .flex_1()
        .min_w_0()
        .child(scroller)
        .children(edge_fades(handle, palette))
}

/// What a scroller inside a pane does with the wheel. The stage scrolls too when the window is
/// smaller than the panes need, and in GPUI every scroller under the pointer applies a turn of
/// the wheel unless one in front of it stops it. A pane's own scroller therefore stops the turns
/// it uses: a turn down over a transcript, the lines of an editor or a terminal moves that and
/// nothing else, and the stage takes the turns over what does not scroll, and the sideways ones
/// over a scroller that only moves down.
pub trait PaneScroll: StatefulInteractiveElement + Styled + Sized {
    /// Scrolls down, and keeps the turns down for itself.
    fn pane_scroll_y(self) -> Self {
        self.overflow_y_scroll().on_scroll_wheel(keep_wheel)
    }

    /// Scrolls sideways, and keeps every turn for itself (a vertical wheel moves it sideways).
    fn pane_scroll_x(self) -> Self {
        self.overflow_x_scroll().on_scroll_wheel(keep_every_wheel)
    }
}

impl<T: StatefulInteractiveElement + Styled> PaneScroll for T {}

/// Whether a turn of the wheel is down or up rather than sideways: the one a scroller that only
/// moves down uses.
pub fn turns_down(delta_x: f32, delta_y: f32) -> bool {
    delta_y.abs() >= delta_x.abs()
}

/// Whether `event` is a turn down or up (see [`turns_down`]).
pub fn is_turn_down(event: &ScrollWheelEvent) -> bool {
    let delta = event.delta.pixel_delta(px(1.0));
    turns_down(f32::from(delta.x), f32::from(delta.y))
}

/// The handler of a pane's scroller that moves only down: the turns down are its own and go no
/// further; a sideways one goes on to the stage.
pub fn keep_wheel(event: &ScrollWheelEvent, _: &mut Window, cx: &mut App) {
    if is_turn_down(event) {
        cx.stop_propagation();
    }
}

/// The handler of a pane's scroller that moves both ways: every turn is its own.
pub fn keep_every_wheel(_: &ScrollWheelEvent, _: &mut Window, cx: &mut App) {
    cx.stop_propagation();
}

/// A canvas that takes no room of its own and tells `report` where its parent was laid out. Put
/// it inside a `relative` parent.
pub fn bounds_probe(report: impl Fn(Bounds<Pixels>, &mut App) + 'static) -> impl IntoElement {
    canvas(move |bounds, _, cx| report(bounds, cx), |_, (), _, _| {})
        .absolute()
        .size_full()
}

/// Whether a sideways scroller hides more at its left end and at its right end. A scroller that
/// has not been laid out yet hides nothing.
pub fn hidden_ends(handle: &ScrollHandle) -> (bool, bool) {
    let offset = f32::from(handle.offset().x);
    let hidden_right = f32::from(handle.max_offset().x) + offset;
    (offset < -FADE_THRESHOLD, hidden_right > FADE_THRESHOLD)
}

/// Whether a scroller hides more above and below. A scroller that has not been laid out yet hides
/// nothing.
pub fn hidden_above_below(handle: &ScrollHandle) -> (bool, bool) {
    let offset = f32::from(handle.offset().y);
    let hidden_below = f32::from(handle.max_offset().y) + offset;
    (offset < -FADE_THRESHOLD, hidden_below > FADE_THRESHOLD)
}

/// A fade of the canvas color over each end of a scroller that hides more, so that a clipped
/// control, or a pane cut off by the window, reads as scrollable: at the left and right ends of a
/// sideways scroller, and above and below one that moves down too. Place the result inside the
/// `relative` parent of the scroller, after it. The handle is the scroller's own `track_scroll`
/// handle.
pub fn edge_fades(handle: &ScrollHandle, palette: &Palette) -> Vec<AnyElement> {
    let (left, right) = hidden_ends(handle);
    let (above, below) = hidden_above_below(handle);
    let solid: Hsla = rgb(palette.canvas).into();
    let clear = Hsla { a: 0.0, ..solid };
    let fade = |angle: f32, from: Hsla, to: Hsla| {
        div().absolute().bg(linear_gradient(
            angle,
            linear_color_stop(from, 0.0),
            linear_color_stop(to, 1.0),
        ))
    };
    let across = |fade: Div| fade.top_0().bottom_0().w(px(desktop::FADE_WIDTH));
    let down = |fade: Div| fade.left_0().right_0().h(px(desktop::FADE_WIDTH));
    let mut fades = Vec::new();
    if left {
        fades.push(across(fade(90.0, solid, clear)).left_0().into_any_element());
    }
    if right {
        fades.push(
            across(fade(90.0, clear, solid))
                .right_0()
                .into_any_element(),
        );
    }
    if above {
        fades.push(down(fade(180.0, solid, clear)).top_0().into_any_element());
    }
    if below {
        fades.push(
            down(fade(180.0, clear, solid))
                .bottom_0()
                .into_any_element(),
        );
    }
    fades
}

#[cfg(test)]
mod tests {
    use super::{not_available, turns_down};

    #[test]
    fn a_scroller_that_moves_down_keeps_the_turns_down_and_lets_the_sideways_ones_go_on() {
        assert!(turns_down(0.0, 12.0));
        assert!(turns_down(0.0, -12.0));
        assert!(turns_down(3.0, -12.0));
        assert!(turns_down(0.0, 0.0));
        assert!(!turns_down(12.0, 0.0));
        assert!(!turns_down(-12.0, 3.0));
    }

    #[test]
    fn one_sentence_says_what_the_desktop_client_does_not_serve() {
        assert_eq!(
            not_available("iPhone"),
            "iPhone is not available in the desktop client yet."
        );
    }

    #[test]
    fn the_web_place_says_the_same_sentence_as_the_iphone_place() {
        // The place draws `not_available(label)` for every place that draws no panes.
        assert_eq!(
            not_available(crate::destination::Destination::Web.label()),
            "Web is not available in the desktop client yet."
        );
    }

    /// Every scroller inside a pane keeps the wheel for itself (`PaneScroll`), or the stage
    /// scrolls with it. Only the files that are not panes may scroll down on their own.
    #[test]
    fn a_scroller_in_a_pane_goes_through_pane_scroll() {
        // `permission.rs` scrolls both ways and keeps every turn with `keep_every_wheel`.
        // `tiles.rs` is the stage itself and `top_bar.rs` the workspace tabs.
        const OUTSIDE_PANES: [&str; 9] = [
            "views/ui.rs",
            "views/permission.rs",
            "views/tiles.rs",
            "views/top_bar.rs",
            "views/preferences.rs",
            "views/places.rs",
            "views/rail.rs",
            "views/menus.rs",
            "views/switch_popover.rs",
        ];
        fn sources(directory: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    sources(&path, found);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    found.push(path);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        sources(&root.join("views"), &mut files);
        for file in files {
            let name = file
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let text = std::fs::read_to_string(&file).unwrap();
            let own = [
                ".overflow_y_scroll()",
                ".overflow_x_scroll()",
                ".overflow_scroll()",
            ]
            .iter()
            .map(|call| text.matches(call).count())
            .sum::<usize>();
            let allowed =
                OUTSIDE_PANES.contains(&name.as_str()) || name.starts_with("views/tasks/");
            assert!(
                own == 0 || allowed,
                "{name} scrolls down without PaneScroll"
            );
        }
    }
}
