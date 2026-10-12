//! What the person does in an agent pane beyond typing and sending: the queue of waiting
//! messages, the sign-in card, the agent's questions, the subagent rows, earlier messages, and
//! telling the engine about a draft.

use crate::{
    composer_field::{ComposerEvent, ComposerField, new_field},
    workbench::Workbench,
};
use gpui::{Context, Entity};
use soyeht_client::{
    Command, Connection,
    agent::TAIL,
    cards::{Earlier, elicitation},
    loops,
};
use std::time::Instant;

impl Workbench {
    /// Tells the engine about every composer that holds text, as often as the Mac does, so that a
    /// loop never sends over what the person is typing.
    pub(crate) fn beat_drafts(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let drafts: Vec<(String, bool)> = self
            .fields
            .iter()
            .map(|(pane_id, field)| (pane_id.clone(), !field.read(cx).draft().text().is_empty()))
            .collect();
        for (pane_id, has_draft) in drafts {
            let last = self.draft_beats.get(&pane_id).copied();
            if !has_draft {
                self.draft_beats.remove(&pane_id);
            } else if loops::draft_beat_due(has_draft, last, now)
                && self.state.connection == Connection::Connected
            {
                self.draft_beats.insert(pane_id.clone(), now);
                self.send(Command::SetDraft {
                    pane_id,
                    active: true,
                });
            }
        }
    }

    pub(crate) fn toggle_subagent(&mut self, subagent_id: &str, cx: &mut Context<Self>) {
        if !self.open_subagents.remove(subagent_id) {
            self.open_subagents.insert(subagent_id.to_owned());
        }
        cx.notify();
    }

    /// How many messages of an agent's transcript the view draws.
    pub(crate) fn transcript_limit(&self, pane_id: &str) -> usize {
        TAIL + self.transcript_extra.get(pane_id).copied().unwrap_or(0)
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
                *self.transcript_extra.entry(pane_id.to_owned()).or_default() += TAIL;
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
        let field = match self.form_fields.get(&handle) {
            Some(field) => field.clone(),
            None => {
                let field = new_field(palette, cx);
                let (pane_id, request_id) = (pane_id.to_owned(), request_id.to_owned());
                cx.subscribe(
                    &field,
                    move |this, _, event: &ComposerEvent, cx| match event {
                        ComposerEvent::Submit => {
                            this.answer_elicitation(&pane_id, &request_id, true, cx)
                        }
                        ComposerEvent::Dismiss => this.close_menu(cx),
                    },
                )
                .detach();
                self.form_fields.insert(handle, field.clone());
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
                self.form_fields
                    .iter()
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
