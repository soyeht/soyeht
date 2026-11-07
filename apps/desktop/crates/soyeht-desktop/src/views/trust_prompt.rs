//! "Trust this folder?": the sheet the window puts over the stage the first time a workspace whose
//! folder has no answer yet is the one in front. Keep restricted is the default answer (the filled
//! button); closing the sheet puts the question off until the application restarts and leaves the
//! folder restricted. The words are the shared client policy's, and which workspace is asked about
//! is the client state's (`State::trust_prompt`).

use super::{prelude::*, sheet};
use crate::theme::{Palette, desktop, layout, pane_interior, space, type_scale};
use gpui::MouseButton;
use soyeht_client::Command;
use soyeht_protocol::{
    ProjectTrustChoice,
    client_policy::{
        PROJECT_TRUST_RESTRICT, PROJECT_TRUST_SUMMARY, PROJECT_TRUST_TITLE, PROJECT_TRUST_TRUST,
    },
};

/// The sheet's width and the most it may be tall in a window of `window` points: as wide as the
/// other sheets of the window, and never more than the window less its margins.
pub(crate) fn sheet_extent(window: (f32, f32)) -> (f32, f32) {
    let room = |side: f32| (side - 2.0 * desktop::PREFERENCES_INSET).max(0.0);
    (
        layout::NEW_TASK_SHEET_WIDTH.min(room(window.0)),
        room(window.1),
    )
}

impl Workbench {
    /// The sheet while the person has to decide about the workspace in front, drawn over the
    /// whole window.
    pub(super) fn trust_prompt_sheet(
        &self,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let workspace = self.state.trust_prompt()?;
        let (width, height) = sheet_extent((self.window.width, self.window.height));
        let header = sheet::header(
            palette,
            PROJECT_TRUST_TITLE,
            workspace.path.clone(),
            row()
                .id("trust-prompt-close")
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.defer_trust_prompt(cx))),
        );
        let buttons = row()
            .flex_wrap()
            .justify_end()
            .gap(px(space::X8))
            .child(choice_button(
                palette,
                "trust-prompt-trust",
                PROJECT_TRUST_TRUST,
                false,
                cx,
                |this, cx| this.answer_trust_prompt(ProjectTrustChoice::Trusted, cx),
            ))
            .child(choice_button(
                palette,
                "trust-prompt-restrict",
                PROJECT_TRUST_RESTRICT,
                true,
                cx,
                |this, cx| this.answer_trust_prompt(ProjectTrustChoice::Restricted, cx),
            ));
        let card = sheet::card(palette, "trust-prompt")
            .w(px(width))
            .max_h(px(height))
            .overflow_y_scroll()
            .text_color(rgb(palette.ink))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(header)
            .child(
                chrome_text(FontWeight::NORMAL, type_scale::CONTROL)
                    .text_color(rgb(palette.secondary))
                    .child(PROJECT_TRUST_SUMMARY),
            )
            .child(buttons);
        Some(
            sheet::centered_layer()
                .id("trust-prompt-overlay")
                .occlude()
                .child(card)
                .into_any_element(),
        )
    }

    /// The person answered: the engine is asked, and the sheet goes when its answer comes.
    fn answer_trust_prompt(&mut self, trust: ProjectTrustChoice, cx: &mut Context<Self>) {
        let Some(workspace_id) = self
            .state
            .trust_prompt()
            .map(|workspace| workspace.workspace_id.clone())
        else {
            return;
        };
        self.send(Command::SetProjectTrust {
            workspace_id,
            trust,
        });
        cx.notify();
    }

    /// The person closed the sheet without answering.
    pub(crate) fn defer_trust_prompt(&mut self, cx: &mut Context<Self>) {
        self.state.defer_trust_prompt();
        cx.notify();
    }
}

/// An answer of the sheet: the default one filled, the other raised.
fn choice_button(
    palette: &'static Palette,
    key: &'static str,
    text: &'static str,
    default: bool,
    cx: &mut Context<Workbench>,
    act: fn(&mut Workbench, &mut Context<Workbench>),
) -> AnyElement {
    sheet::button(palette, key.into(), default)
        .cursor_pointer()
        .active(|style| style.opacity(pane_interior::PRESSED_OPACITY))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| act(this, cx)))
        .child(text)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window_size::MINIMUM;

    #[test]
    fn the_sheet_has_its_width_in_a_window_with_room() {
        let (width, height) = sheet_extent((1280.0, 800.0));
        assert_eq!(width, layout::NEW_TASK_SHEET_WIDTH);
        assert_eq!(height, 800.0 - 2.0 * desktop::PREFERENCES_INSET);
    }

    #[test]
    fn the_sheet_fits_the_smallest_window_the_app_opens_in() {
        let (width, height) = sheet_extent(MINIMUM);
        assert!(width + 2.0 * desktop::PREFERENCES_INSET <= MINIMUM.0);
        assert!(height + 2.0 * desktop::PREFERENCES_INSET <= MINIMUM.1);
        assert!(width > 0.0 && height > 0.0);
    }

    #[test]
    fn the_sheet_shrinks_with_a_window_narrower_than_it_and_never_goes_negative() {
        let (width, _) = sheet_extent((400.0, 440.0));
        assert_eq!(width, 400.0 - 2.0 * desktop::PREFERENCES_INSET);
        assert_eq!(sheet_extent((10.0, 10.0)), (0.0, 0.0));
    }
}
