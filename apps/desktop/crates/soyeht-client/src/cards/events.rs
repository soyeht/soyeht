//! The engine's events and answers that change the cards.

use super::CardState;
use crate::{
    agent::AgentView,
    state::{Effect, State},
};
use serde_json::Value;
use soyeht_protocol::{
    ElicitationRequest, EventName, PaneAuthFinishedEvent, PaneElicitationAnsweredEvent,
    PaneElicitationRequestedEvent, PaneNeedsLoginEvent, PaneQueueStateEvent, PaneQueuedEvent,
    PromptItem,
};

impl State {
    /// A page of the queue of messages waiting for an agent arrived.
    pub(crate) fn apply_queue(&mut self, pane_id: &str, items: Vec<PromptItem>, paused: bool) {
        if let Some(cards) = self.cards_mut(pane_id) {
            cards.queue = items;
            cards.queue_paused = paused;
        }
    }

    /// A question listed by a snapshot's requests.
    pub(crate) fn adopt_elicitation(&mut self, entry: &Value) {
        let Some(request) = entry.get("request") else {
            return;
        };
        match serde_json::from_value::<ElicitationRequest>(request.clone()) {
            Ok(request) => {
                if let Some(cards) = self.cards_mut(&request.pane_id) {
                    cards.elicitation = Some(request);
                }
            }
            Err(error) => {
                self.problem = Some(format!(
                    "An agent's question could not be read ({error}); answer it from another client."
                ));
            }
        }
    }

    /// Events that change the cards. Others are none of this module's business.
    pub(crate) fn card_notification(
        &mut self,
        name: EventName,
        params: Value,
    ) -> serde_json::Result<Vec<Effect>> {
        Ok(match name {
            // Loading a pane's transcript loads its queue with it, as on the Mac.
            EventName::PaneQueued => {
                let event = serde_json::from_value::<PaneQueuedEvent>(params)?;
                if self.agents.contains_key(&event.pane_id) {
                    vec![Effect::LoadTranscript(event.pane_id)]
                } else {
                    Vec::new()
                }
            }
            EventName::PaneQueuePaused | EventName::PaneQueueResumed => {
                let event = serde_json::from_value::<PaneQueueStateEvent>(params)?;
                if let Some(cards) = self.cards_mut(&event.pane_id) {
                    cards.queue_paused = event.paused;
                }
                Vec::new()
            }
            EventName::PaneElicitationRequested => {
                let event = serde_json::from_value::<PaneElicitationRequestedEvent>(params)?;
                if let Some(cards) = self.cards_mut(&event.pane_id) {
                    cards.elicitation = Some(ElicitationRequest {
                        request_id: event.request_id,
                        pane_id: event.pane_id,
                        generation: event.generation,
                        prompt_id: event.prompt_id,
                        title: event.title,
                        schema: event.schema,
                        expires_at: event.expires_at,
                    });
                }
                Vec::new()
            }
            EventName::PaneElicitationAnswered => {
                let event = serde_json::from_value::<PaneElicitationAnsweredEvent>(params)?;
                if let Some(cards) = self.cards_mut(&event.pane_id)
                    && cards
                        .elicitation
                        .as_ref()
                        .is_some_and(|request| request.request_id == event.request_id)
                {
                    cards.elicitation = None;
                }
                Vec::new()
            }
            EventName::PaneNeedsLogin => {
                let event = serde_json::from_value::<PaneNeedsLoginEvent>(params)?;
                if let Some(cards) = self.cards_mut(&event.pane_id) {
                    cards.auth_methods = event.auth_methods;
                }
                vec![Effect::LoadHarnesses]
            }
            EventName::PaneAuthFinished => {
                let event = serde_json::from_value::<PaneAuthFinishedEvent>(params)?;
                if let Some(cards) = self.cards_mut(&event.pane_id) {
                    if event.ok {
                        cards.auth_methods.clear();
                    }
                    cards.sign_in_failure = if event.ok { None } else { event.reason };
                }
                vec![Effect::LoadHarnesses]
            }
            _ => return self.account_notification(name, params),
        })
    }

    /// The person started a sign-in: the failure of the last one no longer applies.
    pub fn begin_sign_in(&mut self, pane_id: &str) {
        if let Some(cards) = self.cards_mut(pane_id) {
            cards.sign_in_failure = None;
        }
    }

    /// The person dismissed the reason a sign-in did not finish.
    pub fn dismiss_sign_in_failure(&mut self, pane_id: &str) {
        self.begin_sign_in(pane_id);
    }

    /// The sign-in choices are gone once the agent restarted on another harness.
    pub(crate) fn clear_sign_in(&mut self, pane_id: &str) {
        if let Some(cards) = self.cards_mut(pane_id) {
            cards.auth_methods.clear();
        }
    }

    fn cards_mut(&mut self, pane_id: &str) -> Option<&mut CardState> {
        self.agents
            .get_mut(pane_id)
            .map(|view: &mut AgentView| &mut view.cards)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Event, Snapshot,
        state::{Effect, State},
        testing::{event, pane, pane_json, workspace},
    };
    use serde_json::{Value, json};
    use soyeht_protocol::EventName;

    fn agent_state() -> State {
        let mut agent = pane_json("a", "w", "agent");
        agent["agent"] = json!({
            "harness": "claude", "version": "1", "fingerprint": "f", "options": [],
            "authMethods": [{"id": "m", "name": "Account", "terminal": true}],
            "queue": [], "draft": false, "role": "worker", "queuePaused": true
        });
        let mut state = State::default();
        state.apply(Event::Connected(Snapshot {
            workspaces: vec![workspace("w")],
            panes: vec![
                serde_json::from_value(agent).unwrap(),
                pane("t", "w", "terminal"),
            ],
            requests: vec![json!({"kind": "elicitation", "request": question("e1")})],
            ..Snapshot::default()
        }));
        state
    }

    fn question(id: &str) -> Value {
        json!({"requestId": id, "paneId": "a", "generation": 1, "promptId": "p", "title": "Which?",
               "schema": {"properties": {"x": {}}}, "expiresAt": "2030-01-01T00:00:00Z"})
    }

    #[test]
    fn a_snapshot_brings_the_question_the_paused_queue_and_the_sign_in_choices() {
        let state = agent_state();
        let cards = &state.agent("a").unwrap().cards;
        assert_eq!(cards.elicitation.as_ref().unwrap().request_id, "e1");
        assert!(cards.queue_paused);
        assert_eq!(cards.auth_methods.len(), 1);
    }

    #[test]
    fn a_question_is_held_until_it_is_answered_and_only_by_its_own_answer() {
        let mut state = agent_state();
        state.apply(event(
            EventName::PaneElicitationRequested,
            json!({"paneId": "a", "requestId": "e2", "generation": 1, "promptId": "p",
                   "title": "Again?", "schema": {}, "expiresAt": "2030-01-01T00:00:00Z"}),
        ));
        let held = |state: &State| {
            state
                .agent("a")
                .unwrap()
                .cards
                .elicitation
                .as_ref()
                .map(|r| r.request_id.clone())
        };
        assert_eq!(held(&state).as_deref(), Some("e2"));
        let answered = |id: &str| {
            event(
                EventName::PaneElicitationAnswered,
                json!({"paneId": "a", "requestId": id, "result": "submitted", "by": "mac"}),
            )
        };
        state.apply(answered("e1"));
        assert_eq!(held(&state).as_deref(), Some("e2"));
        state.apply(answered("e2"));
        assert_eq!(held(&state), None);
    }

    #[test]
    fn the_queue_pauses_and_resumes_and_a_queued_message_reloads_the_pane() {
        let mut state = agent_state();
        let paused = |state: &State| state.agent("a").unwrap().cards.queue_paused;
        state.apply(event(
            EventName::PaneQueueResumed,
            json!({"paneId": "a", "paused": false}),
        ));
        assert!(!paused(&state));
        state.apply(event(
            EventName::PaneQueuePaused,
            json!({"paneId": "a", "paused": true, "reason": "interrupted"}),
        ));
        assert!(paused(&state));
        let effects = state.apply(event(
            EventName::PaneQueued,
            json!({"paneId": "a", "n": 1, "promptIds": ["q"], "draft": false}),
        ));
        assert_eq!(effects, [Effect::LoadTranscript("a".into())]);
        let none = state.apply(event(
            EventName::PaneQueued,
            json!({"paneId": "t", "n": 1, "promptIds": ["q"], "draft": false}),
        ));
        assert!(none.is_empty());
    }

    #[test]
    fn a_sign_in_that_finishes_clears_the_choices_and_one_that_fails_says_why() {
        let mut state = agent_state();
        let failure = |state: &State| state.agent("a").unwrap().cards.sign_in_failure.clone();
        state.apply(event(
            EventName::PaneAuthFinished,
            json!({"paneId": "a", "ok": false, "reason": "Cancelled"}),
        ));
        assert_eq!(failure(&state).as_deref(), Some("Cancelled"));
        assert_eq!(state.agent("a").unwrap().cards.auth_methods.len(), 1);
        state.begin_sign_in("a");
        assert_eq!(failure(&state), None);
        let effects = state.apply(event(
            EventName::PaneAuthFinished,
            json!({"paneId": "a", "ok": true}),
        ));
        assert_eq!(effects, [Effect::LoadHarnesses]);
        assert!(state.agent("a").unwrap().cards.auth_methods.is_empty());
        state.apply(event(
            EventName::PaneNeedsLogin,
            json!({"paneId": "a", "harness": "claude",
                   "authMethods": [{"id": "k", "name": "Key", "terminal": false}]}),
        ));
        assert_eq!(state.agent("a").unwrap().cards.auth_methods[0].id, "k");
    }

    #[test]
    fn a_page_of_the_transcript_says_where_the_earlier_page_starts() {
        let mut state = agent_state();
        let page = |cursor: Option<&str>| Event::Transcript {
            pane_id: "a".into(),
            items: Vec::new(),
            next_cursor: cursor.map(str::to_owned),
        };
        state.apply(page(Some("40")));
        assert!(state.agent("a").unwrap().cards.has_earlier(0));
        state.apply(page(None));
        assert!(!state.agent("a").unwrap().cards.has_earlier(0));
    }

    #[test]
    fn the_queue_the_engine_lists_replaces_the_one_held() {
        let mut state = agent_state();
        let item = json!({"promptId": "q", "text": "go", "origin": "human", "status": "queued",
                          "queuedAt": "2026-10-11T12:00:00Z"});
        state.apply(Event::Queue {
            pane_id: "a".into(),
            items: vec![serde_json::from_value(item).unwrap()],
            paused: false,
        });
        let cards = &state.agent("a").unwrap().cards;
        assert_eq!(cards.queue.len(), 1);
        assert!(!cards.queue_paused);
    }
}
