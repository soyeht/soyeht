//! Engine-owned default names for agent panes.
//!
//! An agent pane created without a name is named after its harness, numbered
//! within its workspace ("Claude", "Claude 2") so task owners and comment
//! authors stay distinguishable. A harness switch renames the pane only while
//! it still carries such a default name; any other name was chosen by a user.

use crate::core::Core;
use soyeht_protocol::HarnessLaunch;
use std::collections::BTreeSet;

/// Name used before agent panes were named after their harness.
const LEGACY_DEFAULT_NAME: &str = "Agent";

pub(crate) fn harness_label(launch: &HarnessLaunch) -> &str {
    if launch.name.trim().is_empty() {
        &launch.id
    } else {
        &launch.name
    }
}

/// Whether `name` is a default the engine assigned for `harness`.
pub(crate) fn is_default_name(name: &str, harness: &str) -> bool {
    if name == LEGACY_DEFAULT_NAME || name == harness {
        return true;
    }
    name.strip_prefix(harness)
        .and_then(|rest| rest.strip_prefix(' '))
        .filter(|number| number.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|number| number.parse::<u32>().ok())
        .is_some_and(|number| number >= 2)
}

impl Core {
    /// The first free default name for `harness` among the workspace's open
    /// panes, ignoring `except` so a pane never collides with itself.
    pub(crate) fn default_agent_name(
        &self,
        workspace_id: &str,
        harness: &str,
        except: Option<&str>,
    ) -> String {
        let taken: BTreeSet<&str> = self
            .panes
            .iter()
            .filter(|(id, running)| {
                Some(id.as_str()) != except
                    && !running.stored.closed
                    && running.stored.pane.workspace_id == workspace_id
            })
            .map(|(_, running)| running.stored.pane.name.as_str())
            .collect();
        let mut name = harness.to_owned();
        let mut number = 1;
        while taken.contains(name.as_str()) {
            number += 1;
            name = format!("{harness} {number}");
        }
        name
    }
}

#[cfg(test)]
mod tests {
    use super::is_default_name;

    #[test]
    fn default_names_are_the_harness_name_with_an_optional_ordinal() {
        assert!(is_default_name("Claude", "Claude"));
        assert!(is_default_name("Claude 2", "Claude"));
        assert!(is_default_name("Agent", "Claude"));
        assert!(!is_default_name("Claude 1", "Claude"));
        assert!(!is_default_name("Claude 2b", "Claude"));
        assert!(!is_default_name("Claudette", "Claude"));
        assert!(!is_default_name("Reviewer", "Claude"));
    }
}
