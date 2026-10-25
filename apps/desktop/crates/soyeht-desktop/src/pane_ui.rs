//! What the window keeps for each agent pane besides what the engine says about it: the message
//! line, the scroll positions, the files waiting to be sent, the panel half filled in, and which
//! thoughts and subagents the person has opened. All of it hangs off one `PaneUi` per pane, so a
//! pane that closes takes its interface state with it in one step, and a new kind of state has
//! no list to remember.

use crate::{
    composer_field::{ComposerEvent, ComposerField, new_field},
    editor_ui::EditorUi,
    loop_form::LoopForm,
    views::Replies,
    workbench::Workbench,
    writing_ui::WritingUi,
};
use gpui::{Context, Entity, ScrollHandle};
use soyeht_client::{
    State, attachments::Staged, pane_close::Closing, slash, text_input::keys::Completion,
    transcript_rows::Rows,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Instant,
};

#[derive(Default)]
pub(crate) struct PaneUi {
    /// The message line of the pane's composer, made when first needed.
    pub field: Option<Entity<ComposerField>>,
    /// The scroll of the transcript and how many messages it had when last laid out, to follow
    /// the newest while it grows.
    pub transcript: Option<(ScrollHandle, usize)>,
    /// What the rows of the transcript were laid out at, so that a long one builds only the rows
    /// near the viewport.
    pub rows: Rc<RefCell<Rows>>,
    /// The sideways scroll of the composer's chip strip.
    pub chip_scroll: ScrollHandle,
    /// The sideways scroll of the composer's attachment chips.
    pub attachment_scroll: ScrollHandle,
    /// How many messages beyond `TAIL` the transcript draws.
    pub extra: usize,
    /// The files attached to the pane's next message, with the latest refusal.
    pub staged: Option<Staged>,
    /// The composer's "/" list: which row is chosen and which query it was closed for.
    pub slash: slash::List,
    /// The loop panel the pane is editing.
    pub loop_form: Option<LoopForm>,
    /// When the composer's draft was last announced to the engine.
    pub draft_beat: Option<Instant>,
    /// The thoughts the person has opened, by message.
    pub open_thoughts: HashSet<String>,
    /// The subagent rows the person has opened, by subagent.
    pub open_subagents: HashSet<String>,
    /// The fields of the questions the agent asks, by request and field.
    pub form_fields: HashMap<(String, String), Entity<ComposerField>>,
    /// What an editor pane keeps: the scroll, the find bar, the tabs' and the panel's scroll.
    pub editor: EditorUi,
    /// What a terminal pane keeps: where its screen is, a drag, the address under the pointer.
    pub terminal: crate::terminal_input::TerminalUi,
    /// What a text pane keeps: the field that holds the text and its scroll.
    pub writing: WritingUi,
    /// Closing the pane waits for the person's answer to this question.
    pub closing: Option<Closing>,
}

impl PaneUi {
    /// Drops what refers to messages and requests the pane no longer holds.
    fn forget_gone(&mut self, state: &State, pane_id: &str) {
        let Some(view) = state.agent(pane_id) else {
            return;
        };
        self.form_fields.retain(|(request, _), _| {
            view.cards
                .elicitation
                .as_ref()
                .is_some_and(|open| &open.request_id == request)
        });
        if !self.open_thoughts.is_empty() {
            self.open_thoughts
                .retain(|message| view.messages.iter().any(|m| &m.local_id == message));
        }
    }
}

/// The `PaneUi` of every pane that has one.
#[derive(Default)]
pub(crate) struct PaneUis(HashMap<String, PaneUi>);

impl PaneUis {
    pub fn get(&self, pane_id: &str) -> Option<&PaneUi> {
        self.0.get(pane_id)
    }

    pub fn get_mut(&mut self, pane_id: &str) -> Option<&mut PaneUi> {
        self.0.get_mut(pane_id)
    }

    pub fn thought_is_open(&self, pane_id: &str, message_id: &str) -> bool {
        self.get(pane_id)
            .is_some_and(|ui| ui.open_thoughts.contains(message_id))
    }

    pub fn subagent_is_open(&self, pane_id: &str, subagent_id: &str) -> bool {
        self.get(pane_id)
            .is_some_and(|ui| ui.open_subagents.contains(subagent_id))
    }

    /// The pane's state, made when first needed.
    pub fn of(&mut self, pane_id: &str) -> &mut PaneUi {
        self.0.entry(pane_id.to_owned()).or_default()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &PaneUi)> {
        self.0.iter()
    }

    /// Drops the state of panes that are gone, with the parse of their Markdown preview, and of
    /// messages and requests that are.
    pub fn forget_closed(&mut self, state: &State, markdown: &mut Replies) {
        self.0.retain(|pane_id, _| {
            let open = state.panes().iter().any(|pane| &pane.pane_id == pane_id);
            if !open {
                markdown.forget_preview(pane_id);
            }
            open
        });
        for (pane_id, ui) in &mut self.0 {
            ui.forget_gone(state, pane_id);
        }
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// What each workspace's editor place keeps of its project panel.
#[derive(Default)]
pub(crate) struct ProjectPanels {
    /// The workspaces whose editors have their project panel folded away.
    pub(crate) hidden: HashSet<String>,
    /// The scroll of the project panel of each workspace's editor place that has no file open.
    pub(crate) tree_scroll: HashMap<String, ScrollHandle>,
}

impl Workbench {
    /// Drops what is kept per pane for panes that are gone, and per message for messages that are.
    pub(crate) fn forget_closed_panes(&mut self) {
        self.panes.forget_closed(&self.state, &mut self.markdown);
        let open = |id: &String| {
            self.state
                .workspaces()
                .iter()
                .any(|workspace| &workspace.workspace_id == id)
        };
        let (hidden, scrolls): (HashSet<String>, HashMap<String, ScrollHandle>) = (
            self.panels
                .hidden
                .iter()
                .filter(|id| open(id))
                .cloned()
                .collect(),
            self.panels
                .tree_scroll
                .iter()
                .filter(|(id, _)| open(id))
                .map(|(id, handle)| (id.clone(), handle.clone()))
                .collect(),
        );
        self.panels.hidden = hidden;
        self.panels.tree_scroll = scrolls;
    }

    /// The message line of an agent's composer, made when first needed.
    pub(crate) fn composer_field(
        &mut self,
        pane_id: &str,
        cx: &mut Context<Self>,
    ) -> Entity<ComposerField> {
        if let Some(field) = self.panes.get(pane_id).and_then(|ui| ui.field.as_ref()) {
            return field.clone();
        }
        let field = new_field(self.theme.palette(), cx);
        let owner = pane_id.to_owned();
        cx.subscribe(
            &field,
            move |this, _, event: &ComposerEvent, cx| match event {
                ComposerEvent::Submit => this.submit_prompt(&owner, cx),
                ComposerEvent::Dismiss => this.dismiss_composer(&owner, cx),
                ComposerEvent::Attach => {
                    if let Some(item) = cx.read_from_clipboard() {
                        this.paste_into_composer(&owner, &item, cx);
                    }
                }
            },
        )
        .detach();
        let owner = pane_id.to_owned();
        cx.subscribe(&field, move |this, _, event: &Completion, cx| {
            this.complete_slash(&owner, *event, cx)
        })
        .detach();
        self.panes.of(pane_id).field = Some(field.clone());
        field
    }

    /// The sideways scroll of an agent composer's chip strip, made when first needed.
    pub(crate) fn chip_scroll(&mut self, pane_id: &str) -> ScrollHandle {
        self.panes.of(pane_id).chip_scroll.clone()
    }

    /// The sideways scroll of an agent composer's attachment chips, made when first needed.
    pub(crate) fn attachment_scroll(&mut self, pane_id: &str) -> ScrollHandle {
        self.panes.of(pane_id).attachment_scroll.clone()
    }

    /// The scroll state of an agent's transcript, following the newest message while it grows;
    /// true when this frame scrolls it to the newest.
    pub(crate) fn transcript_scroll(
        &mut self,
        pane_id: &str,
        messages: usize,
    ) -> (ScrollHandle, bool) {
        let (handle, seen) = self
            .panes
            .of(pane_id)
            .transcript
            .get_or_insert_with(|| (ScrollHandle::new(), 0));
        let follows = *seen != messages;
        if follows {
            *seen = messages;
            handle.scroll_to_bottom();
        }
        (handle.clone(), follows)
    }

    /// What the rows of an agent's transcript measured, made when first needed.
    pub(crate) fn transcript_rows(&mut self, pane_id: &str) -> Rc<RefCell<Rows>> {
        self.panes.of(pane_id).rows.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use soyeht_client::{Event, Snapshot};
    use std::time::Instant;

    fn state_with(agents: &[&str]) -> State {
        let pane = |id: &str| {
            json!({
                "paneId": id, "workspaceId": "w", "name": id, "cwd": "/work", "mode": "agent",
                "state": "idle", "source": "none", "confidence": 0.0, "phrase": "",
                "generation": 1, "cols": 80, "rows": 24, "record": false,
                "agent": {
                    "harness": "worker-harness", "version": "1", "fingerprint": "f", "options": [],
                    "authMethods": [], "queue": [], "draft": false, "role": "worker"
                }
            })
        };
        let snapshot = Snapshot {
            workspaces: vec![
                serde_json::from_value(
                    json!({"workspaceId": "w", "name": "w", "path": "/work", "layout": {}}),
                )
                .unwrap(),
            ],
            panes: agents
                .iter()
                .map(|id| serde_json::from_value(pane(id)).unwrap())
                .collect(),
            focus: None,
            cursor: 1,
            ..Snapshot::default()
        };
        let mut state = State::default();
        state.apply(Event::Connected(snapshot));
        state
    }

    /// Fills every field of a `PaneUi` that does not need a window. Written as a struct
    /// expression with no `..Default::default()`, so a field added to `PaneUi` fails to compile
    /// here until the test fills it too.
    fn used() -> PaneUi {
        PaneUi {
            field: None,
            transcript: Some((ScrollHandle::new(), 3)),
            rows: Rc::default(),
            chip_scroll: ScrollHandle::new(),
            attachment_scroll: ScrollHandle::new(),
            extra: 50,
            staged: Some(Staged::default()),
            slash: slash::List::default(),
            loop_form: None,
            draft_beat: Some(Instant::now()),
            open_thoughts: HashSet::from(["thought".to_owned()]),
            open_subagents: HashSet::from(["subagent".to_owned()]),
            form_fields: HashMap::new(),
            editor: EditorUi::default(),
            terminal: Default::default(),
            writing: WritingUi::default(),
            closing: Some(Closing::Immediately),
        }
    }

    #[test]
    fn a_closed_pane_leaves_no_state_of_its_own() {
        let mut uis = PaneUis::default();
        *uis.of("kept") = used();
        *uis.of("closed") = used();
        let mut markdown = Replies::default();
        let preview = markdown.preview_blocks("closed", "# one");
        uis.forget_closed(&state_with(&["kept"]), &mut markdown);
        assert!(!std::sync::Arc::ptr_eq(
            &preview,
            &markdown.preview_blocks("closed", "# one")
        ));
        assert!(uis.get("closed").is_none());
        let kept = uis.get("kept").unwrap();
        assert_eq!(kept.extra, 50);
        assert!(kept.open_subagents.contains("subagent"));
        uis.forget_closed(&state_with(&[]), &mut Replies::default());
        assert!(uis.is_empty());
    }

    #[test]
    fn a_thought_of_a_message_that_is_gone_is_forgotten() {
        let mut uis = PaneUis::default();
        *uis.of("kept") = used();
        uis.forget_closed(&state_with(&["kept"]), &mut Replies::default());
        assert!(uis.get("kept").unwrap().open_thoughts.is_empty());
    }
}
