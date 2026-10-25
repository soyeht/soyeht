//! When closing a pane asks first. The cases are `contracts/vectors/pane-close.json`, which the
//! Mac and iOS clients pass too (`PaneClosePolicy`).

use soyeht_protocol::PaneMode;

/// What closing a pane asks first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Closing {
    Immediately,
    Confirm { title: String, message: String },
}

/// The question closing a pane of `mode` asks. A pane that runs a process always asks, as the
/// Mac does, whether or not anything is running in it. A document and an editor tab ask by
/// rules of their own (`writing::rules::closing`, the editor's conflict with the disk), which
/// need their file, so for them this is `Immediately`.
pub fn confirmation(mode: &PaneMode) -> Closing {
    match mode {
        PaneMode::Terminal | PaneMode::Agent | PaneMode::Web => Closing::Confirm {
            title: "Close pane?".to_owned(),
            message: "The process in this pane will be terminated.".to_owned(),
        },
        PaneMode::Editor | PaneMode::Writing => Closing::Immediately,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{cases, text};

    #[test]
    fn a_pane_asks_as_the_vector_says() {
        for case in cases("pane-close", "panes") {
            let mode: PaneMode = serde_json::from_value(case["mode"].clone()).unwrap();
            let expected = case["confirm"]
                .as_object()
                .map_or(Closing::Immediately, |confirm| Closing::Confirm {
                    title: text(&confirm["title"]).to_owned(),
                    message: text(&confirm["message"]).to_owned(),
                });
            assert_eq!(confirmation(&mode), expected, "{}", case["name"]);
        }
    }

    #[test]
    fn the_vector_names_every_kind_of_pane() {
        let named: Vec<PaneMode> = cases("pane-close", "panes")
            .iter()
            .map(|case| serde_json::from_value(case["mode"].clone()).unwrap())
            .collect();
        for mode in [
            PaneMode::Terminal,
            PaneMode::Agent,
            PaneMode::Web,
            PaneMode::Writing,
            PaneMode::Editor,
        ] {
            assert!(named.contains(&mode), "{mode:?}");
        }
    }
}
