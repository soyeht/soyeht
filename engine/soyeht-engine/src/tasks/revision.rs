//! Pull request status and repository-diff boundary for task rules.

use crate::errors::Result;
use soyeht_protocol::{TaskCheckState, TaskReviewState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SurfaceKind {
    PublicFunction,
    Handler,
    ProtocolMessage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedSurface {
    pub path: String,
    pub name: String,
    pub kind: SurfaceKind,
    pub patch: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RevisionSnapshot {
    pub head: String,
    pub check_state: TaskCheckState,
    pub review_state: TaskReviewState,
    pub summary: String,
    pub log_tail: String,
    pub fingerprint: String,
    pub changed_surfaces: Vec<ChangedSurface>,
}

pub trait RevisionPort: Send {
    fn inspect(&mut self, url: &str) -> Result<RevisionSnapshot>;
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::errors;
    use std::collections::{BTreeMap, VecDeque};

    #[derive(Default)]
    pub struct ScriptedRevisionPort {
        scripts: BTreeMap<String, VecDeque<RevisionSnapshot>>,
    }

    impl ScriptedRevisionPort {
        pub fn script(&mut self, url: &str, snapshots: Vec<RevisionSnapshot>) {
            self.scripts.insert(url.into(), snapshots.into());
        }
    }

    impl RevisionPort for ScriptedRevisionPort {
        fn inspect(&mut self, url: &str) -> Result<RevisionSnapshot> {
            self.scripts
                .get_mut(url)
                .and_then(VecDeque::pop_front)
                .ok_or_else(|| errors::internal("Revision fixture has no scripted result"))
        }
    }

    #[test]
    fn scripted_port_returns_check_and_review_states_in_order() {
        let url = "https://example.invalid/change/1";
        let mut port = ScriptedRevisionPort::default();
        let snapshot = |check_state, review_state, fingerprint: &str| RevisionSnapshot {
            head: "abc123".into(),
            check_state,
            review_state,
            summary: "A scripted result".into(),
            log_tail: "last line".into(),
            fingerprint: fingerprint.into(),
            changed_surfaces: Vec::new(),
        };
        port.script(
            url,
            vec![
                snapshot(TaskCheckState::Red, TaskReviewState::None, "red-1"),
                snapshot(
                    TaskCheckState::Green,
                    TaskReviewState::AwaitingPerson,
                    "green-1",
                ),
            ],
        );

        assert_eq!(port.inspect(url).unwrap().check_state, TaskCheckState::Red);
        let green = port.inspect(url).unwrap();
        assert_eq!(green.check_state, TaskCheckState::Green);
        assert_eq!(green.review_state, TaskReviewState::AwaitingPerson);
        assert!(port.inspect(url).is_err());
    }
}
