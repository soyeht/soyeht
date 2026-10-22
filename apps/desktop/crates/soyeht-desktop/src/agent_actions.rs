//! What the person does in an agent pane beyond typing and sending: the queue of waiting
//! messages, the sign-in card, the agent's questions, the subagent rows, earlier messages, and
//! telling the engine about a draft.

use crate::{
    composer_field::{ComposerEvent, ComposerField, new_field},
    views::selectable::selected_text,
    workbench::Workbench,
};
use gpui::{ClipboardItem, Context, Entity};
use soyeht_client::{
    Command, Connection,
    agent::TAIL,
    cards::{Earlier, elicitation},
    loops,
};
use std::time::Instant;

impl Workbench {
    /// Puts the text selected in a transcript on the clipboard. False when nothing is selected.
    pub(crate) fn copy_transcript(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(text) = selected_text(cx) else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    /// Tells the engine about every composer that holds text, as often as the Mac does, so that a
    /// loop never sends over what the person is typing.
    pub(crate) fn beat_drafts(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let drafts: Vec<(String, bool)> = self
            .panes
            .iter()
            .filter_map(|(pane_id, ui)| {
                let field = ui.field.as_ref()?;
                Some((pane_id.clone(), !field.read(cx).draft().text().is_empty()))
            })
            .collect();
        for (pane_id, has_draft) in drafts {
            let ui = self.panes.of(&pane_id);
            let last = ui.draft_beat;
            if !has_draft {
                ui.draft_beat = None;
            } else if loops::draft_beat_due(has_draft, last, now)
                && self.state.connection == Connection::Connected
            {
                ui.draft_beat = Some(now);
                self.send(Command::SetDraft {
                    pane_id,
                    active: true,
                });
            }
        }
    }

    pub(crate) fn toggle_subagent(
        &mut self,
        pane_id: &str,
        subagent_id: &str,
        cx: &mut Context<Self>,
    ) {
        let open = &mut self.panes.of(pane_id).open_subagents;
        if !open.remove(subagent_id) {
            open.insert(subagent_id.to_owned());
        }
        cx.notify();
    }

    /// How many messages of an agent's transcript the view draws.
    pub(crate) fn transcript_limit(&self, pane_id: &str) -> usize {
        TAIL + self.panes.get(pane_id).map_or(0, |ui| ui.extra)
    }

    /// "Load earlier messages": draws the messages already held beyond the last ones, and when
    /// all of them are drawn asks the engine for the page before the oldest.
    pub(crate) fn load_earlier(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let limit = self.transcript_limit(pane_id);
        let Some(view) = self.state.agent(pane_id) else {
            return;
        };
        match view.cards.earlier(view.tail(limit).1) {
            Earlier::Reveal => {
                self.panes.of(pane_id).extra += TAIL;
            }
            Earlier::Fetch(before) => {
                self.send(Command::LoadOlder {
                    pane_id: pane_id.to_owned(),
                    before,
                });
            }
            Earlier::Nothing => {}
        }
        cx.notify();
    }

    pub(crate) fn remove_queued(&mut self, pane_id: &str, prompt_id: &str) {
        self.send(Command::RemoveQueued {
            pane_id: pane_id.to_owned(),
            prompt_id: prompt_id.to_owned(),
        });
    }

    pub(crate) fn resume_queue(&mut self, pane_id: &str) {
        self.send(Command::ResumeQueue {
            pane_id: pane_id.to_owned(),
        });
    }

    pub(crate) fn start_sign_in(
        &mut self,
        pane_id: &str,
        method_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.state.begin_sign_in(pane_id);
        self.send(Command::StartSignIn {
            pane_id: pane_id.to_owned(),
            method_id,
        });
        cx.notify();
    }

    pub(crate) fn cancel_sign_in(&mut self, pane_id: &str) {
        self.send(Command::CancelSignIn {
            pane_id: pane_id.to_owned(),
        });
    }

    pub(crate) fn dismiss_sign_in(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        self.state.dismiss_sign_in_failure(pane_id);
        cx.notify();
    }

    /// The field of an agent's question for `key`, made when first needed. Return in a field
    /// answers the question.
    pub(crate) fn form_field(
        &mut self,
        pane_id: &str,
        request_id: &str,
        key: &str,
        title: &str,
        cx: &mut Context<Self>,
    ) -> Entity<ComposerField> {
        let palette = self.theme.palette();
        let handle = (request_id.to_owned(), key.to_owned());
        let field = match self
            .panes
            .get(pane_id)
            .and_then(|ui| ui.form_fields.get(&handle))
        {
            Some(field) => field.clone(),
            None => {
                let field = new_field(palette, cx);
                let (owner, request_id) = (pane_id.to_owned(), request_id.to_owned());
                cx.subscribe(
                    &field,
                    move |this, _, event: &ComposerEvent, cx| match event {
                        ComposerEvent::Submit => {
                            this.answer_elicitation(&owner, &request_id, true, cx)
                        }
                        ComposerEvent::Dismiss => this.close_menu(cx),
                        ComposerEvent::Attach => {}
                    },
                )
                .detach();
                self.panes
                    .of(pane_id)
                    .form_fields
                    .insert(handle, field.clone());
                field
            }
        };
        field.update(cx, |field, _| field.style(palette, title));
        field
    }

    /// Answers the agent's question with what the person typed, or declines it.
    pub(crate) fn answer_elicitation(
        &mut self,
        pane_id: &str,
        request_id: &str,
        submit: bool,
        cx: &mut Context<Self>,
    ) {
        let fields = submit.then(|| {
            elicitation::answer(
                self.panes
                    .get(pane_id)
                    .into_iter()
                    .flat_map(|ui| ui.form_fields.iter())
                    .filter(|((request, _), _)| request == request_id)
                    .map(|((_, key), field)| {
                        (key.clone(), field.read(cx).draft().text().to_owned())
                    }),
            )
        });
        self.send(Command::AnswerElicitation {
            pane_id: pane_id.to_owned(),
            request_id: request_id.to_owned(),
            cancel: fields.is_none(),
            fields,
        });
        cx.notify();
    }
}
