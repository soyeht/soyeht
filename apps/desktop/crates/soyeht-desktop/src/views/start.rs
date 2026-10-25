//! "Start with an agent": the Mac's new-pane sheet in one card. A message line on top, and under
//! it the folder, the agent and the round start button, as the composer lays its controls out.
//! The rules are `soyeht_client::new_agent`; the flow is `start_flow`.

use super::{
    composer::{ControlChip, card_frame, ink_button},
    harness_menu::{Pick, Picker},
    menus,
    ui::{caption, chrome_text, empty_state, icon, logo, pane_card_shell, pressable, row},
};
use crate::assets::{self, Icon};
use crate::{
    composer_field::focus_on_press,
    open_menu::Menu,
    theme::{Palette, composer, desktop, type_scale},
    workbench::Workbench,
};
use gpui::{AnyElement, Context, FontWeight, SharedString, div, px, rgb};
use gpui::{InteractiveElement, IntoElement, ParentElement, Styled};
use soyeht_client::{Connection, new_agent::NewAgent, state::name_of};
use std::rc::Rc;

const DETAIL: &str = "Pick a folder and an agent, then describe it. An app, a presentation, a \
                      study, a script: the agent works in the folder.";

impl Workbench {
    pub(super) fn start_form(
        &mut self,
        cx: &mut Context<Self>,
        palette: &'static Palette,
    ) -> Option<AnyElement> {
        let form = self.start_form_now(cx)?;
        let field = self.start.as_ref()?.field.clone();
        field.update(cx, |field, _| {
            field.style(palette, "Describe what you want to make");
        });
        let blocker = form.blocker(self.state.harnesses());
        let can_start = self.state.connection == Connection::Connected && blocker.is_none();
        let (folder, agent) = self.start_chips(&form, palette, cx);
        let card = focus_on_press(
            card_frame(palette)
                .w_full()
                .max_w(px(desktop::START_FORM_WIDTH)),
            &field,
        )
        .child(
            div()
                .min_h(px(desktop::START_PROMPT_MIN_HEIGHT))
                .child(field),
        )
        .child(
            row()
                .gap(px(composer::CONTROL_SPACING))
                .child(folder)
                .child(agent)
                .child(div().flex_1())
                .child(start_button(can_start, palette, cx)),
        );
        let note = blocker.map_or("Enter starts. Shift+Enter adds a line.", |blocker| {
            blocker.explanation()
        });
        let cancel = pressable(
            row().id("start-cancel").cursor_pointer(),
            true,
            cx,
            |this, cx| this.close_start(cx),
        )
        .child(
            chrome_text(FontWeight::SEMIBOLD, type_scale::SMALL)
                .text_color(rgb(palette.secondary))
                .child("Cancel"),
        );
        let footer = row()
            .w_full()
            .max_w(px(desktop::START_FORM_WIDTH))
            .justify_between()
            .child(caption(palette).child(SharedString::from(note)))
            .child(cancel);
        Some(
            pane_card_shell(palette)
                .child(
                    empty_state(
                        palette,
                        Some(Icon::Cpu),
                        "What do you want to make?",
                        Some(DETAIL.into()),
                    )
                    .child(card)
                    .child(footer),
                )
                .into_any_element(),
        )
    }

    /// The folder chip, with the folders to start in, and the agent chip, with the agents
    /// installed on this machine grouped by how ready they are.
    fn start_chips(
        &mut self,
        form: &NewAgent,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> (AnyElement, AnyElement) {
        let tall = desktop::SWITCH_POPOVER_MAXIMUM_HEIGHT;
        let folder_popup = self.menus.is_open(&Menu::StartFolder).then(|| {
            let choose = menus::item(
                palette,
                "start-folder-choose",
                "Choose folder\u{2026}",
                false,
            );
            let mut rows = vec![menus::on_pick(choose, cx, Workbench::choose_start_folder)];
            rows.extend(menus::folder_rows(
                ("start-folder", &form.directory),
                palette,
                self.state.recent_folders(),
                cx,
                Workbench::set_start_folder,
            ));
            (menus::list(rows), tall)
        });
        let agent_popup = self.menus.is_open(&Menu::StartAgent).then(|| {
            let pick: Pick = Rc::new(|this, harness, cx| this.set_start_harness(harness, cx));
            let picker = Picker {
                chosen: Some(form.harness.as_str()),
                ..Picker::default()
            };
            (
                menus::list(self.harness_rows(picker, palette, &pick, cx)),
                tall,
            )
        });
        let folder_name = match form.directory.as_str() {
            "" => "Choose folder".to_owned(),
            directory => name_of(directory),
        };
        let agent_name = self
            .state
            .harness_name(&form.harness)
            .unwrap_or("Choose agent");
        let agent_mark = assets::logo_path(&form.harness)
            .map(|path| logo(path, composer::HARNESS_LOGO_SIZE, palette.ink).into_any_element());
        let folder = ControlChip {
            key: "start-folder-chip".into(),
            label: &folder_name,
            emphasized: false,
            leading: Some(
                icon(Icon::Folder, composer::ICON_SIZE, palette.secondary).into_any_element(),
            ),
            enabled: true,
            menu: Menu::StartFolder,
            popup: folder_popup,
        };
        let agent = ControlChip {
            key: "start-agent-chip".into(),
            label: agent_name,
            emphasized: true,
            leading: agent_mark,
            enabled: true,
            menu: Menu::StartAgent,
            popup: agent_popup,
        };
        (folder.build(palette, cx), agent.build(palette, cx))
    }
}

/// The round button that starts the agent: ink on the face, dimmed while the form is not ready.
fn start_button(
    enabled: bool,
    palette: &'static Palette,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    pressable(
        ink_button("start-agent", enabled, palette),
        enabled,
        cx,
        |this, cx| this.submit_start(cx),
    )
    .child(icon(Icon::ArrowUp, desktop::COMPOSER_GLYPH, palette.face))
    .into_any_element()
}
