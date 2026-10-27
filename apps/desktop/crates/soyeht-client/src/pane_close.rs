//! When closing a pane or a workspace asks first. The cases are `contracts/vectors/pane-close.json`,
//! which the Mac and iOS clients pass too (`PaneClosePolicy`).

use soyeht_protocol::PaneMode;

/// What closing a pane or a workspace asks first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Closing {
    Immediately,
    /// The question, and the word on the button that answers yes.
    Confirm {
        title: String,
        message: String,
        confirm_title: String,
    },
}

impl Closing {
    /// A question whose yes is "Close", as every question about a pane has.
    pub fn asking(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Confirm {
            title: title.into(),
            message: message.into(),
            confirm_title: "Close".to_owned(),
        }
    }
}

/// Where a workspace's folder lives, as its question says it: on this computer, since the
/// engine runs here. The Mac and iOS clients say "the Mac" (the vector's `places`).
const FOLDER_PLACE: &str = "this computer";

/// The question closing a workspace asks. Closing it closes every pane in it, so it asks
/// whenever it holds one, whether or not anything runs there; a workspace with no pane closes at
/// once. Its folder and files are not touched. Only a person's own close asks: the engine's
/// `workspace.close` never does.
pub fn workspace_confirmation(name: &str, pane_count: usize) -> Closing {
    if pane_count == 0 {
        return Closing::Immediately;
    }
    let panes = if pane_count == 1 {
        "its pane".to_owned()
    } else {
        format!("its {pane_count} panes")
    };
    Closing::Confirm {
        title: format!("Close {name}?"),
        message: format!(
            "The processes in {panes} will be terminated. The folder stays on {FOLDER_PLACE}."
        ),
        confirm_title: "Close workspace".to_owned(),
    }
}

/// The question closing a pane of `mode` asks. A pane that runs a process always asks, as the
/// Mac does, whether or not anything is running in it. A document and an editor tab ask by
/// rules of their own (`writing::rules::closing`, the editor's conflict with the disk), which
/// need their file, so for them this is `Immediately`.
pub fn confirmation(mode: &PaneMode) -> Closing {
    match mode {
        PaneMode::Terminal | PaneMode::Agent | PaneMode::Web => Closing::asking(
            "Close pane?",
            "The process in this pane will be terminated.",
        ),
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
            assert_eq!(confirmation(&mode), expected(&case, ""), "{}", case["name"]);
        }
    }

    #[test]
    fn a_workspace_asks_as_the_vector_says() {
        let place = text(&crate::testing::vector("pane-close")["places"]["desktop"]).to_owned();
        let cases = cases("pane-close", "workspaces");
        assert!(!cases.is_empty());
        for case in cases {
            let count = usize::try_from(case["paneCount"].as_u64().unwrap()).unwrap();
            assert_eq!(
                workspace_confirmation(text(&case["workspaceName"]), count),
                expected(&case, &place),
                "{}",
                case["name"]
            );
        }
    }

    /// What the vector says a case asks, with `{place}` read as `place`.
    fn expected(case: &serde_json::Value, place: &str) -> Closing {
        case["confirm"]
            .as_object()
            .map_or(Closing::Immediately, |confirm| Closing::Confirm {
                title: text(&confirm["title"]).to_owned(),
                message: text(&confirm["message"]).replace("{place}", place),
                confirm_title: text(&confirm["confirmTitle"]).to_owned(),
            })
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
