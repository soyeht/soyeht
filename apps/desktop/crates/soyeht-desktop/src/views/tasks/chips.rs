//! The small pieces the board, the list and the detail share: the chips of a task's state,
//! priority and owner, and the colors of the roles.

use crate::{
    theme::{Palette, desktop, pane_interior, space, type_scale},
    views::prelude::*,
};
use soyeht_client::tasks::{self, Plate, Role};
use soyeht_protocol::{TaskPriority, TaskState};

/// The color a role is painted in.
pub(super) fn role_color(role: Role, palette: &Palette) -> u32 {
    match role {
        Role::Accent => palette.accent,
        Role::Secondary => palette.secondary,
        Role::Warning => palette.warning(),
        Role::Success => palette.success(),
        Role::Danger => palette.danger(),
    }
}

/// The plate an owner's name is painted on: the palette's agent plate, or the face when the
/// palette has fewer.
pub(super) fn plate_color(plate: Plate, palette: &Palette) -> u32 {
    palette
        .agent_plates
        .get(plate.index())
        .copied()
        .unwrap_or(palette.face)
}

/// A small bold word in a capsule of `fill`.
pub(super) fn capsule(fill: u32, text: impl Into<SharedString>, color: u32) -> Div {
    div()
        .flex_none()
        .px(px(space::X8))
        .py(px(space::X2))
        .rounded_full()
        .bg(rgb(fill))
        .chrome()
        .font_weight(FontWeight::BOLD)
        .text_size(px(type_scale::MICRO))
        .text_color(rgb(color))
        .child(text.into())
}

pub(super) fn priority_chip(priority: &TaskPriority, palette: &Palette) -> Div {
    capsule(
        palette.well,
        tasks::priority_title(priority),
        palette.secondary,
    )
}

/// The priority as a card says it: in lower case.
pub(super) fn priority_tag(priority: &TaskPriority, palette: &Palette) -> Div {
    capsule(
        palette.well,
        tasks::priority_title(priority).to_lowercase(),
        palette.secondary,
    )
}

/// The state's name in the state's color; work in hand is the accent filled.
pub(super) fn state_chip(state: &TaskState, palette: &Palette) -> Div {
    let name = tasks::state_title(state);
    if *state == TaskState::InProgress {
        capsule(palette.accent, name, palette.on_accent)
    } else {
        capsule(
            palette.well,
            name,
            role_color(tasks::state_role(state), palette),
        )
    }
}

/// The owner: a dot and the name on the owner's plate.
pub(super) fn owner_chip(name: &str, palette: &Palette) -> Div {
    row()
        .flex_none()
        .gap(px(space::X5))
        .pl(px(space::X6))
        .pr(px(space::X8))
        .py(px(space::X3))
        .rounded_full()
        .bg(rgb(plate_color(tasks::owner_plate(name), palette)))
        .mono()
        .font_weight(FontWeight::BOLD)
        .text_size(px(type_scale::MICRO))
        .text_color(rgb(palette.ink))
        .child(
            div()
                .size(px(pane_interior::COMPACT_SPACING - space::X2))
                .rounded_full()
                .bg(rgb(palette.ink)),
        )
        .child(div().truncate().child(SharedString::from(name.to_owned())))
}

/// A dot of `size` points in a role's color.
pub(super) fn dot(role: Role, size: f32, palette: &Palette) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .bg(rgb(role_color(role, palette)))
}

/// A mono word in the muted ink: a key, a branch.
pub(super) fn muted_mono(text: impl Into<SharedString>, palette: &Palette) -> Div {
    div()
        .mono()
        .font_weight(FontWeight::BOLD)
        .text_size(px(type_scale::SMALL))
        .text_color(rgb(palette.muted))
        .child(text.into())
}

/// The glyph size of the small icons of a card.
pub(super) const CARD_GLYPH: f32 = desktop::INLINE_GLYPH - 2.0;

/// How many tasks a group holds, in a well capsule.
pub(super) fn count_pill(palette: &Palette, count: usize) -> Div {
    div()
        .px(px(space::X7))
        .rounded_full()
        .bg(rgb(palette.well))
        .text_size(px(type_scale::MICRO))
        .child(count.to_string())
}

/// The title of a task.
pub(super) fn task_title(task: &soyeht_protocol::Task, palette: &Palette) -> Div {
    div()
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(type_scale::CAPTION))
        .text_color(rgb(palette.ink))
        .child(SharedString::from(task.title.clone()))
}

/// A dot and a line of words in the secondary ink: where a task stands. `lines` is how many
/// lines the words may take, none meaning one that is cut short.
pub(super) fn status_row(
    role: Role,
    text: String,
    dot_size: f32,
    lines: Option<usize>,
    palette: &Palette,
) -> Div {
    let words = div().flex_1().min_w_0();
    row()
        .gap(px(space::X6))
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(type_scale::SMALL))
        .text_color(rgb(palette.secondary))
        .child(dot(role, dot_size, palette))
        .child(
            match lines {
                Some(lines) => words.line_clamp(lines),
                None => words.truncate(),
            }
            .child(SharedString::from(text)),
        )
}

/// The speech bubble and how many comments a task has.
pub(super) fn comment_count(count: usize, palette: &Palette) -> Div {
    row()
        .gap(px(space::X4))
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(type_scale::MICRO))
        .text_color(rgb(palette.muted))
        .child(icon(Icon::Comment, CARD_GLYPH, palette.muted))
        .child(count.to_string())
}

/// A well that holds a few lines: a declaration list, what must not change, who needs you.
pub(super) fn well_card(palette: &Palette) -> Div {
    div()
        .p(px(space::X12))
        .rounded(px(crate::theme::radius::COMPACT))
        .bg(rgb(palette.well))
}
