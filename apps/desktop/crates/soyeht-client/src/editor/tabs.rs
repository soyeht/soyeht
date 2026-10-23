//! Which editor tab shows, and which takes over when one closes (`EditorTabPolicy` on the Mac;
//! the cases are `contracts/vectors/editor.json`, which both clients pass).
//!
//! A tab is a pane, in the order the engine keeps. `history` lists the panes this client
//! activated, newest first.

use soyeht_protocol::{EditorTab, client_policy::EDITOR_TAB_HISTORY};

/// A preview stays a preview while its file keeps the revision it was previewed at: the first
/// edit makes it a permanent tab. A file the client has not read yet cannot have changed.
pub fn is_preview(tab: &EditorTab, file_revision: Option<&str>) -> bool {
    tab.preview
        && tab
            .preview_revision
            .as_deref()
            .is_some_and(|revision| file_revision.is_none_or(|current| current == revision))
}

/// The tab that shows: the selected pane when it is a tab, else the most recently activated
/// tab, else the first.
pub fn active(selected: Option<&str>, tabs: &[String], history: &[String]) -> Option<String> {
    if let Some(selected) = selected.filter(|selected| tabs.iter().any(|tab| tab == selected)) {
        return Some(selected.to_owned());
    }
    history
        .iter()
        .find(|pane| tabs.contains(pane))
        .or_else(|| tabs.first())
        .cloned()
}

/// The tab that takes over when `closing` closes: the previously active one, else the right
/// neighbour, else the left one.
pub fn successor(closing: &str, tabs: &[String], history: &[String]) -> Option<String> {
    let remaining = || tabs.iter().filter(|tab| *tab != closing);
    if let Some(recent) = history
        .iter()
        .find(|pane| *pane != closing && remaining().any(|tab| tab == *pane))
    {
        return Some(recent.clone());
    }
    let Some(index) = tabs.iter().position(|tab| tab == closing) else {
        return remaining().next().cloned();
    };
    tabs.get(index + 1)
        .or_else(|| index.checked_sub(1).and_then(|before| tabs.get(before)))
        .cloned()
}

/// The next or previous tab, wrapping around.
pub fn cycled(current: Option<&str>, tabs: &[String], forward: bool) -> Option<String> {
    if tabs.is_empty() {
        return None;
    }
    let Some(index) = current.and_then(|current| tabs.iter().position(|tab| tab == current)) else {
        return if forward { tabs.first() } else { tabs.last() }.cloned();
    };
    let step = if forward { 1 } else { tabs.len() - 1 };
    tabs.get((index + step) % tabs.len()).cloned()
}

/// `pane` first in the activation history, which stays bounded.
pub fn recorded(pane: &str, history: &[String]) -> Vec<String> {
    std::iter::once(pane.to_owned())
        .chain(history.iter().filter(|other| *other != pane).cloned())
        .take(EDITOR_TAB_HISTORY as usize)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{strings, text, vector};
    use serde_json::Value;

    fn name(value: &Value) -> Option<String> {
        value.as_str().map(str::to_owned)
    }

    fn tabs() -> Value {
        vector("editor")["tabs"].clone()
    }

    fn each(key: &str) -> Vec<Value> {
        tabs()[key].as_array().unwrap().clone()
    }

    #[test]
    fn the_active_tab_is_the_one_the_vector_says() {
        for case in each("active") {
            let selected = name(&case["selected"]);
            let shown = active(
                selected.as_deref(),
                &strings(&case["tabs"]),
                &strings(&case["history"]),
            );
            assert_eq!(shown, name(&case["tab"]), "{}", case["name"]);
        }
    }

    #[test]
    fn a_closing_tab_hands_over_as_the_vector_says() {
        for case in each("successor") {
            let next = successor(
                text(&case["closing"]),
                &strings(&case["tabs"]),
                &strings(&case["history"]),
            );
            assert_eq!(next, name(&case["tab"]), "{}", case["name"]);
        }
    }

    #[test]
    fn cycling_wraps_as_the_vector_says() {
        for case in each("cycled") {
            let current = name(&case["current"]);
            let next = cycled(
                current.as_deref(),
                &strings(&case["tabs"]),
                case["forward"].as_bool().unwrap(),
            );
            assert_eq!(next, name(&case["tab"]), "{}", case["name"]);
        }
    }

    #[test]
    fn the_history_puts_the_newest_first_and_stays_bounded() {
        for case in each("recorded") {
            let after = recorded(text(&case["pane"]), &strings(&case["history"]));
            assert_eq!(after, strings(&case["historyAfter"]), "{}", case["name"]);
        }
        let cap = &tabs()["historyCap"];
        let many: Vec<String> = (0..cap["historyCount"].as_u64().unwrap())
            .map(|index| format!("p{index}"))
            .collect();
        let capped = recorded(text(&cap["pane"]), &many);
        assert_eq!(capped.len() as u64, cap["length"].as_u64().unwrap());
        assert_eq!(capped[0], text(&cap["pane"]));
    }

    #[test]
    fn a_preview_stays_one_until_the_file_changes() {
        for case in each("preview") {
            let tab = EditorTab {
                pane_id: "p".into(),
                file_id: "f".into(),
                path: "a.rs".into(),
                preview: case["preview"].as_bool().unwrap(),
                preview_revision: name(&case["previewRevision"]),
            };
            let revision = name(&case["fileRevision"]);
            assert_eq!(
                is_preview(&tab, revision.as_deref()),
                case["isPreview"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
        }
    }
}
