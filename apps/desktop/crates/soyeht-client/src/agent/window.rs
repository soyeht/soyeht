//! How much of a transcript the view draws, and what "Load earlier messages" does about the rest.

use super::{AgentView, TAIL};
use crate::{Command, cards::Earlier};

/// How many of an agent's last messages the view draws.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TranscriptWindow {
    /// How many messages beyond `TAIL` are drawn.
    extra: usize,
}

impl TranscriptWindow {
    /// How many messages of the transcript the view draws.
    pub fn limit(&self) -> usize {
        TAIL + self.extra
    }

    /// Draws one more page of the messages the client already holds.
    pub fn reveal_page(&mut self) {
        self.extra += TAIL;
    }

    /// "Load earlier messages": draws the messages already held beyond the last ones, and when all
    /// of them are drawn asks the engine for the page before the oldest.
    pub fn load_earlier(&mut self, pane_id: &str, view: &AgentView) -> Option<Command> {
        match view.cards.earlier(view.tail(self.limit()).1) {
            Earlier::Reveal => {
                self.reveal_page();
                None
            }
            Earlier::Fetch(before) => Some(Command::LoadOlder {
                pane_id: pane_id.to_owned(),
                before,
            }),
            Earlier::Nothing => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn view_of_messages(count: usize, older: Option<&str>) -> AgentView {
        let mut view = AgentView::default();
        for index in 0..count {
            let item = json!({"localId": format!("m{index}"), "generation": 1,
                "seq": (index + 1).to_string(), "role": "user", "kind": "message",
                "text": "x", "complete": true});
            view.merge(serde_json::from_value(item).unwrap()).unwrap();
        }
        view.cards.older = older.map(str::to_owned);
        view
    }

    #[test]
    fn the_view_draws_the_last_page_until_it_is_asked_for_more() {
        let mut window = TranscriptWindow::default();
        assert_eq!(window.limit(), TAIL);
        window.reveal_page();
        assert_eq!(window.limit(), 2 * TAIL);
    }

    #[test]
    fn messages_held_and_not_drawn_are_drawn_before_the_engine_is_asked() {
        let view = view_of_messages(TAIL + 5, Some("1"));
        let mut window = TranscriptWindow::default();
        assert_eq!(window.load_earlier("a", &view), None);
        assert_eq!(window.limit(), 2 * TAIL);
    }

    #[test]
    fn when_everything_held_is_drawn_the_engine_is_asked_for_the_page_before() {
        let view = view_of_messages(3, Some("1"));
        let mut window = TranscriptWindow::default();
        assert_eq!(
            window.load_earlier("a", &view),
            Some(Command::LoadOlder {
                pane_id: "a".into(),
                before: "1".into(),
            })
        );
        assert_eq!(window.limit(), TAIL, "asking the engine reveals nothing");
    }

    #[test]
    fn a_transcript_with_nothing_earlier_asks_for_nothing() {
        let view = view_of_messages(3, None);
        let mut window = TranscriptWindow::default();
        assert_eq!(window.load_earlier("a", &view), None);
        assert_eq!(window.limit(), TAIL);
    }
}
