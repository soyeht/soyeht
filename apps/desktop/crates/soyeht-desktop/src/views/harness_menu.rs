//! The rows of the two harness pickers, the "+" menu that opens an agent and the composer chip
//! that switches one, laid out as the Mac's `AgentHarnessPicker` does: the harnesses installed
//! on this machine in groups by readiness, each with how ready it is on a second line, then
//! every other one under a collapsed "More harnesses" entry. A harness that cannot start is
//! listed there, dimmed, and does nothing when pressed.

use super::{menus, ui::icon};
use crate::{
    assets::Icon,
    theme::{Palette, desktop},
    workbench::Workbench,
};
use gpui::{AnyElement, Context, IntoElement};
use soyeht_client::harness_picker::{self, Group, MORE_TITLE};
use soyeht_protocol::HarnessInfo;
use std::rc::Rc;

/// What picking the harness `id` does.
pub type Pick = Rc<dyn Fn(&mut Workbench, &str, &mut Context<Workbench>)>;

/// What a harness picker lists: every harness but `excluding` (the one a pane already runs: it
/// cannot be switched to) that answers to `query`, with `chosen` marked.
#[derive(Clone, Copy, Default)]
pub(super) struct Picker<'a> {
    pub excluding: Option<&'a str>,
    pub query: &'a str,
    pub chosen: Option<&'a str>,
    /// A harness that cannot start can still be picked, to read why (the switch popover does:
    /// it disables its button instead). Otherwise it is dimmed and does nothing.
    pub pick_unavailable: bool,
}

impl Workbench {
    /// The rows of the open harness picker, or the sentence that says why there are none.
    pub(super) fn harness_rows(
        &self,
        picker: Picker,
        palette: &'static Palette,
        pick: &Pick,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let all = self.state.harnesses();
        let layout = harness_picker::layout(all, picker.query, picker.excluding);
        if layout.sections.is_empty() && layout.more.is_empty() {
            return vec![empty_note(&self.state, picker, palette)];
        }
        let mut rows = Vec::new();
        for (group, items) in &layout.sections {
            rows.push(menus::heading(palette, group.title()).into_any_element());
            rows.extend(
                items
                    .iter()
                    .map(|harness| harness_row(harness, picker, palette, pick, cx)),
            );
        }
        if !layout.more.is_empty() {
            let expanded = harness_picker::more_expanded(
                &layout,
                self.menus.more_harnesses(),
                !picker.query.trim().is_empty(),
                picker.chosen,
            );
            rows.push(menus::on_pick(
                menus::disclosure(
                    palette,
                    "more-harnesses",
                    &format!("{MORE_TITLE} \u{b7} {}", layout.more.len()),
                    expanded,
                ),
                cx,
                |this, cx| this.toggle_more_harnesses(cx),
            ));
            if expanded {
                rows.extend(
                    layout
                        .more
                        .iter()
                        .map(|harness| harness_row(harness, picker, palette, pick, cx)),
                );
            }
        }
        rows
    }
}

/// Why a picker shows no harness: they have not arrived, there are none, the search matches
/// nothing, or the only match is the harness the pane already runs.
fn empty_note(
    state: &soyeht_client::State,
    picker: Picker,
    palette: &'static Palette,
) -> AnyElement {
    let text = if !state.harnesses_loaded() {
        "Loading agents\u{2026}".to_owned()
    } else if state
        .harnesses()
        .iter()
        .all(|harness| Some(harness.id.as_str()) == picker.excluding)
    {
        "No agents available. Rescan after installing or configuring an agent.".to_owned()
    } else if let Some(running) = state
        .harnesses()
        .iter()
        .find(|harness| Some(harness.id.as_str()) == picker.excluding)
        .filter(|harness| harness_picker::matches(harness, picker.query))
    {
        format!(
            "{} is already active. Choose another agent to restart this conversation.",
            running.name
        )
    } else {
        format!("No agents match \u{201c}{}\u{201d}.", picker.query.trim())
    };
    menus::note(palette, text).into_any_element()
}

fn harness_row(
    harness: &HarnessInfo,
    picker: Picker,
    palette: &'static Palette,
    pick: &Pick,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let row = menus::detail_item(
        palette,
        &harness.id,
        readiness_mark(harness_picker::group_of(harness), palette),
        harness.name.clone(),
        harness_picker::status_label(harness),
        picker.chosen == Some(harness.id.as_str()),
        harness.available || picker.pick_unavailable,
    );
    if !harness.available && !picker.pick_unavailable {
        return row.into_any_element();
    }
    let (id, pick) = (harness.id.clone(), Rc::clone(pick));
    menus::on_pick(row, cx, move |this, cx| pick(this, &id, cx))
}

/// The mark before a harness row, in the Mac's colors: a check while it is ready, an alert while
/// it needs the person, and a muted circle or dash while it is unchecked or not installed.
fn readiness_mark(group: Group, palette: &Palette) -> AnyElement {
    let (glyph, color) = match group {
        Group::Ready => (Icon::Check, palette.success()),
        Group::NotChecked => (Icon::Circle, palette.muted),
        Group::SignIn | Group::CheckSetup => (Icon::Alert, palette.warning()),
        Group::NotInstalled => (Icon::Minimize, palette.muted),
    };
    icon(glyph, desktop::INLINE_GLYPH, color).into_any_element()
}
