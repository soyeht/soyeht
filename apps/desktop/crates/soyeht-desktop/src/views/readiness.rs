//! The readiness checklist: what this machine has of what Soyeht needs, and the command that
//! installs each thing it lacks. Preferences shows it as the Setup panel and the welcome screen
//! shows it while something is missing, as the Mac's `ReadinessChecklistView` does.

use super::{
    preferences::{page, pill_button},
    switch_popover::well_row,
    ui::{Typeface, chrome_text, column, icon, icon_button, id, row, section_label},
};
use crate::{
    assets::Icon,
    theme::{Palette, desktop, space, type_scale},
    workbench::Workbench,
};
use gpui::{AnyElement, ClickEvent, ClipboardItem, Context, Div, FontWeight, SharedString};
use gpui::{div, prelude::*, px, rgb};
use soyeht_client::{
    Command,
    readiness::{Row, RowKind, RowState},
};

const SUMMARY: &str =
    "What Soyeht needs on this machine, and the command that installs what is missing.";

impl Workbench {
    pub(super) fn setup_page(&mut self, palette: &'static Palette, cx: &mut Context<Self>) -> Div {
        let content = self.readiness_list(palette, cx);
        page(palette, "Setup", Some(SUMMARY), None, content)
    }

    /// The headline with its "Check again", then the rows: the engine and the tools, then the
    /// agents.
    pub(super) fn readiness_list(&self, palette: &'static Palette, cx: &mut Context<Self>) -> Div {
        let checklist = self.state.readiness();
        let basics = checklist
            .rows
            .iter()
            .filter(|row| row.kind != RowKind::Harness);
        let agents = checklist
            .rows
            .iter()
            .filter(|row| row.kind == RowKind::Harness);
        let basics: Vec<AnyElement> = basics.map(|row| readiness_row(row, palette, cx)).collect();
        let agents: Vec<AnyElement> = agents.map(|row| readiness_row(row, palette, cx)).collect();
        let heading = chrome_text(FontWeight::BOLD, type_scale::SUBHEADING)
            .child(SharedString::from(checklist.headline.clone()));
        let refresh = pill_button(
            palette,
            "readiness-refresh".into(),
            Some(Icon::Refresh),
            "Check again",
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, _| {
            this.send(Command::LoadReadiness);
        }));
        column()
            .gap(px(space::X8))
            .child(row().justify_between().child(heading).child(refresh))
            .child(column().gap(px(space::X4)).children(basics))
            .when(!agents.is_empty(), |list| {
                list.child(
                    div()
                        .pt(px(space::X6))
                        .child(section_label(palette, "AGENTS \u{b7} ONE IS ENOUGH")),
                )
                .child(column().gap(px(space::X4)).children(agents))
            })
    }
}

fn readiness_row(item: &Row, palette: &'static Palette, cx: &mut Context<Workbench>) -> AnyElement {
    let needed = item.required || item.kind == RowKind::Harness;
    let (glyph, color) = match (item.state, needed) {
        (RowState::Ready, _) => (Icon::Check, palette.success()),
        (RowState::Missing, true) => (Icon::Alert, palette.warning()),
        (RowState::Missing, false) => (Icon::Circle, palette.muted),
    };
    let optional = !item.required && item.kind == RowKind::Tool;
    let command = |hint: &String| {
        let copied = hint.clone();
        let tip = "Copy the command";
        let button = icon_button(
            id("readiness-copy", &item.id),
            Icon::Copy,
            palette.secondary,
            tip,
            palette,
        );
        let text = div()
            .mono()
            .truncate()
            .text_size(px(type_scale::CAPTION))
            .text_color(rgb(palette.secondary))
            .child(SharedString::from(hint.clone()));
        let copy = cx.listener(move |_, _: &ClickEvent, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
        });
        (text, button.on_click(copy))
    };
    well_row(palette)
        .py(px(space::X6))
        .id(id("readiness-row", &item.id))
        .gap(px(space::X10))
        .child(icon(glyph, desktop::INLINE_GLYPH, color))
        .child(
            column()
                .child(
                    chrome_text(FontWeight::SEMIBOLD, type_scale::CAPTION)
                        .child(SharedString::from(item.title.clone())),
                )
                .when_some(item.note.clone(), |title, note| {
                    title.child(
                        chrome_text(FontWeight::NORMAL, type_scale::MICRO)
                            .text_color(rgb(palette.muted))
                            .child(SharedString::from(note)),
                    )
                }),
        )
        .when(optional, |line| {
            line.child(
                chrome_text(FontWeight::NORMAL, type_scale::MICRO)
                    .text_color(rgb(palette.muted))
                    .child("optional"),
            )
        })
        .child(div().flex_1().min_w_0())
        .when_some(item.hint.as_ref(), |line, hint| {
            let (text, copy) = command(hint);
            line.child(text).child(copy)
        })
        .into_any_element()
}
