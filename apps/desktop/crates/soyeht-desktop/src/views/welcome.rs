//! What the stage says when the engine has no workspace yet: a workspace is a folder, and the
//! way to start is to ask an agent to make something in a folder or to open one.

use super::{prelude::*, ui::empty_state};
use crate::{
    folder::SHORTCUT,
    neo,
    theme::{Palette, desktop, elevation, layout, radius, space, type_scale},
};

impl Workbench {
    pub(super) fn welcome(&self, palette: &'static Palette, cx: &mut Context<Self>) -> AnyElement {
        let start = action(
            palette,
            ("welcome-start-agent", Icon::Cpu, "Start with an agent"),
            cx.listener(|this, _: &ClickEvent, _, cx| this.open_start(cx)),
        );
        let button = action(
            palette,
            ("welcome-open-folder", Icon::Folder, "Open a folder"),
            cx.listener(|this, _: &ClickEvent, _, cx| this.choose_folder(cx)),
        );
        empty_state(
            palette,
            Some(Icon::Cpu),
            "What do you want to make?",
            Some(
                "Pick a folder, choose an agent and describe it. Or open a folder: a workspace is a folder, and Soyeht opens a terminal in it."
                    .into(),
            ),
        )
        .child(start)
        .child(button)
        .child(
            chrome_text(FontWeight::MEDIUM, type_scale::SMALL)
                .text_color(rgb(palette.muted))
                .child(SHORTCUT),
        )
        .when(!self.state.readiness().complete, |stage| {
            stage.child(
                self.readiness_list(palette, cx)
                    .w(px(layout::READINESS_WIDTH)),
            )
        })
        .into_any_element()
    }
}

/// A raised pill of the welcome screen: an icon and what it does, in the accent.
fn action(
    palette: &'static Palette,
    (key, glyph, label): (&'static str, Icon, &'static str),
    on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    neo::raised(palette, radius::PILL, elevation::PILL)
        .id(key)
        .cursor_pointer()
        .h(px(layout::TOP_BAR_CONTROL_HEIGHT))
        .px(px(space::X18))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(space::X8))
        .own_press()
        .on_click(on_click)
        .child(icon(glyph, desktop::INLINE_GLYPH, palette.accent))
        .child(
            chrome_text(FontWeight::SEMIBOLD, type_scale::CONTROL)
                .text_color(rgb(palette.accent))
                .child(label),
        )
}
