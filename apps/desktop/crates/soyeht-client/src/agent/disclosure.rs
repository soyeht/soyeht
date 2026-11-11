//! What the person has opened in an agent's transcript: thoughts, subagent rows and further pages
//! of long replies. The engine knows none of it, and it goes when the message it is about goes.

use super::AgentView;
use std::collections::{HashMap, HashSet};

/// The openings of one agent pane.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Disclosures {
    /// The thoughts opened, by message.
    thoughts: HashSet<String>,
    /// The subagent rows opened, by subagent.
    subagents: HashSet<String>,
    /// How many pages of a long reply were asked for, by message (see `markdown::shown`).
    reply_pages: HashMap<String, usize>,
}

/// Opens `id` when it is closed, closes it when it is open.
fn toggle(open: &mut HashSet<String>, id: &str) {
    if !open.remove(id) {
        open.insert(id.to_owned());
    }
}

impl Disclosures {
    pub fn toggle_thought(&mut self, message_id: &str) {
        toggle(&mut self.thoughts, message_id);
    }

    pub fn thought_is_open(&self, message_id: &str) -> bool {
        self.thoughts.contains(message_id)
    }

    pub fn toggle_subagent(&mut self, subagent_id: &str) {
        toggle(&mut self.subagents, subagent_id);
    }

    pub fn subagent_is_open(&self, subagent_id: &str) -> bool {
        self.subagents.contains(subagent_id)
    }

    /// "Show more" under a long reply: its next page.
    pub fn show_more_of_reply(&mut self, message_id: &str) {
        *self.reply_pages.entry(message_id.to_owned()).or_insert(1) += 1;
    }

    /// The pages of the reply that are drawn: the first, until the person asks for more.
    pub fn reply_pages(&self, message_id: &str) -> usize {
        self.reply_pages.get(message_id).copied().unwrap_or(1)
    }

    /// Drops what refers to messages the transcript no longer holds.
    pub fn forget_gone(&mut self, view: &AgentView) {
        let held = |message: &String| view.messages().iter().any(|m| &m.local_id == message);
        if !self.thoughts.is_empty() {
            self.thoughts.retain(held);
        }
        self.reply_pages.retain(|message, _| held(message));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_thought_and_a_subagent_open_and_close_with_the_same_click() {
        let mut open = Disclosures::default();
        assert!(!open.thought_is_open("m") && !open.subagent_is_open("s"));
        open.toggle_thought("m");
        open.toggle_subagent("s");
        assert!(open.thought_is_open("m") && open.subagent_is_open("s"));
        open.toggle_thought("m");
        assert!(!open.thought_is_open("m") && open.subagent_is_open("s"));
    }

    #[test]
    fn each_show_more_draws_one_page_further() {
        let mut open = Disclosures::default();
        assert_eq!(open.reply_pages("m"), 1);
        open.show_more_of_reply("m");
        open.show_more_of_reply("m");
        assert_eq!(open.reply_pages("m"), 3);
        assert_eq!(open.reply_pages("other"), 1);
    }

    #[test]
    fn what_refers_to_a_message_that_is_gone_is_forgotten() {
        let mut view = AgentView::default();
        let item = json!({"localId": "kept", "generation": 1, "seq": "1", "role": "user",
                          "kind": "message", "text": "x", "complete": true});
        view.merge(serde_json::from_value(item).unwrap()).unwrap();
        let mut open = Disclosures::default();
        for message in ["kept", "gone"] {
            open.toggle_thought(message);
            open.show_more_of_reply(message);
        }
        open.toggle_subagent("s");
        open.forget_gone(&view);
        assert!(open.thought_is_open("kept") && !open.thought_is_open("gone"));
        assert_eq!((open.reply_pages("kept"), open.reply_pages("gone")), (2, 1));
        assert!(open.subagent_is_open("s"), "subagents are not messages");
    }
}
