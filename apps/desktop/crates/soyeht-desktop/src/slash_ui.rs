//! The composer's "/" list as the window runs it: the rows for what is written, the keys that
//! move the choice and put it in the message line, and Escape closing it. The rules (which rows,
//! in which order, what choosing leaves) are `soyeht_client::slash`'s; this only connects them
//! to the message line.

use crate::workbench::Workbench;
use gpui::Context;
use soyeht_client::{
    slash::{self, Suggestion},
    text_input::keys::Completion,
};

impl Workbench {
    /// The rows of the pane's "/" list for what its message line holds; none while the line is
    /// not a `/` and one word, or the person closed the list for this query.
    pub(crate) fn slash_rows(&mut self, pane_id: &str, cx: &mut Context<Self>) -> Vec<Suggestion> {
        let Some(field) = self.panes.get(pane_id).and_then(|ui| ui.field.clone()) else {
            return Vec::new();
        };
        let catalog = self.state.slash_catalog(pane_id);
        let text = field.read(cx).draft().text();
        self.panes.of(pane_id).slash.rows(&catalog, text)
    }

    /// The row of the pane's list that Return, Tab or a click would put in the line.
    pub(crate) fn slash_selection(&self, pane_id: &str, rows: &[Suggestion]) -> Option<usize> {
        self.panes.get(pane_id)?.slash.selected(rows)
    }

    /// A key the open list took from the message line.
    pub(crate) fn complete_slash(
        &mut self,
        pane_id: &str,
        key: Completion,
        cx: &mut Context<Self>,
    ) {
        let rows = self.slash_rows(pane_id, cx);
        match key {
            Completion::Step(by) => self.panes.of(pane_id).slash.step(by, rows.len()),
            Completion::Accept => {
                if let Some(at) = self.slash_selection(pane_id, &rows) {
                    self.choose_slash(pane_id, &rows[at], cx);
                }
            }
        }
        cx.notify();
    }

    /// Puts a row of the list in the message line: `/name `, ready for the arguments.
    pub(crate) fn choose_slash(
        &mut self,
        pane_id: &str,
        suggestion: &Suggestion,
        cx: &mut Context<Self>,
    ) {
        if let Some(field) = self.panes.get(pane_id).and_then(|ui| ui.field.clone()) {
            let text = slash::accept(suggestion);
            field.update(cx, |field, cx| field.replace(&text, cx));
        }
        cx.notify();
    }

    /// Escape in the message line: closes the list when one is open, else the open menu.
    pub(crate) fn dismiss_composer(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        if self.slash_rows(pane_id, cx).is_empty() {
            self.close_menu(cx);
            return;
        }
        if let Some(field) = self.panes.get(pane_id).and_then(|ui| ui.field.clone()) {
            let text = field.read(cx).draft().text().to_owned();
            self.panes.of(pane_id).slash.dismiss(&text);
        }
        cx.notify();
    }
}
