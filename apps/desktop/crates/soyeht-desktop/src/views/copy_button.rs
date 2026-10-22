//! The Copy button of the transcript: a small icon that appears over a message, a code block or a
//! command's output while the pointer is over it, puts its text on the clipboard and shows a check
//! for a moment. The Mac makes the same text selectable (`.textSelection`); here a message and a
//! block are copied whole, and `selectable` text is copied with the keyboard.

use super::ui::{icon_button, id};
use crate::{
    assets::Icon,
    theme::{Palette, pane_interior},
};
use gpui::{
    App, ClickEvent, ClipboardItem, Div, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window,
};
use std::time::Duration;

/// How long the check shows after a copy.
const COPIED_FOR: Duration = Duration::from_millis(1500);

/// The button for `text`. `key` names it among the buttons of the window.
#[derive(IntoElement)]
struct CopyButton {
    key: SharedString,
    text: SharedString,
    palette: &'static Palette,
}

impl RenderOnce for CopyButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let copied = window.use_keyed_state(self.key.clone(), cx, |_, _| false);
        let done = *copied.read(cx);
        let (glyph, tip, color) = if done {
            (Icon::Check, "Copied", self.palette.success())
        } else {
            (Icon::Copy, "Copy", self.palette.secondary)
        };
        let text = self.text;
        icon_button(id("copy", &self.key), glyph, color, tip, self.palette).on_click(
            move |_: &ClickEvent, _, cx: &mut App| {
                cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
                copied.update(cx, |state, cx| {
                    *state = true;
                    cx.notify();
                });
                let copied = copied.clone();
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(COPIED_FOR).await;
                    // Once the window is gone the update fails, and nobody waits for the check.
                    copied.update(cx, |state, cx| {
                        *state = false;
                        cx.notify();
                    })
                })
                .detach();
            },
        )
    }
}

/// `block` with a Copy button of `text` in its top right corner, shown while the pointer is over
/// the block. `inset` is how far the button sits from the corner.
pub(super) fn copyable(
    block: Div,
    key: &str,
    text: impl Into<SharedString>,
    inset: f32,
    palette: &'static Palette,
) -> Div {
    let group = SharedString::from(format!("copyable-{key}"));
    let button = CopyButton {
        key: SharedString::from(key.to_owned()),
        text: text.into(),
        palette,
    };
    block.relative().group(group.clone()).child(
        gpui::div()
            .absolute()
            .top(gpui::px(inset))
            .right(gpui::px(inset))
            .p(gpui::px(pane_interior::TIGHT_SPACING))
            .rounded_full()
            .bg(gpui::rgb(palette.well))
            .opacity(0.0)
            .group_hover(group, |style| style.opacity(1.0))
            .child(button),
    )
}
