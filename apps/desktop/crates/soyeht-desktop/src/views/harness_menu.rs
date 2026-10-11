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

impl Workbench {
    /// The rows of the open harness picker, every harness but `excluding` (the one a pane
    /// already runs: it cannot be switched to).
    pub(super) fn harness_rows(
        &self,
        excluding: Option<&str>,
        palette: &'static Palette,
        pick: &Pick,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let listed: Vec<HarnessInfo> = self
            .state
            .harnesses()
            .iter()
            .filter(|harness| Some(harness.id.as_str()) != excluding)
            .cloned()
            .collect();
        let layout = harness_picker::layout(&listed);
        let mut rows = Vec::new();
        for (group, items) in &layout.sections {
            rows.push(menus::heading(palette, group.title()).into_any_element());
            rows.extend(
                items
                    .iter()
                    .map(|harness| harness_row(harness, palette, pick, cx)),
            );
        }
        if !layout.more.is_empty() {
            let expanded = harness_picker::more_expanded(&layout, self.menus.more_harnesses());
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
                        .map(|harness| harness_row(harness, palette, pick, cx)),
                );
            }
        }
        rows
    }
}

fn harness_row(
    harness: &HarnessInfo,
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
        harness.available,
    );
    if !harness.available {
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
