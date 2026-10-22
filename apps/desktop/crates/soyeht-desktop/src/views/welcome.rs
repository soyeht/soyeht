//! What the stage says when the engine has no workspace yet: a workspace is a folder, and the
//! way to start is to open one.

use super::{prelude::*, ui::empty_state};
use crate::{
    folder::SHORTCUT,
    neo,
    theme::{Palette, desktop, elevation, layout, radius, space, type_scale},
};

impl Workbench {
    pub(super) fn welcome(&self, palette: &'static Palette, cx: &mut Context<Self>) -> AnyElement {
        let button = neo::raised(palette, radius::PILL, elevation::PILL)
            .id("welcome-open-folder")
            .cursor_pointer()
            .h(px(layout::TOP_BAR_CONTROL_HEIGHT))
            .px(px(space::X18))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(space::X8))
            .own_press()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.choose_folder(cx)))
            .child(icon(Icon::Folder, desktop::INLINE_GLYPH, palette.accent))
            .child(
                chrome_text(FontWeight::SEMIBOLD, type_scale::CONTROL)
                    .text_color(rgb(palette.accent))
                    .child("Open a folder"),
            );
        empty_state(
            palette,
            Some(Icon::Folder),
            "Open a folder to start",
            Some("A workspace is a folder. Soyeht opens a terminal in it.".into()),
        )
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
