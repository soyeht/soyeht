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
/// Registry names describe the ACP adapter ("Claude Agent"). In a pane that is
/// already an agent the word adds nothing, so the pane is named "Claude".
const ADAPTER_SUFFIX: &str = " Agent";

/// The harness name an agent pane is named after.
pub(crate) fn harness_label(launch: &HarnessLaunch) -> &str {
    let name = launch.name.trim();
    if name.is_empty() {
        return &launch.id;
    }
    name.strip_suffix(ADAPTER_SUFFIX)
        .map(str::trim_end)
        .filter(|label| !label.is_empty())
        .unwrap_or(name)
}

/// Whether `name` is `base` or `base N` with N >= 2.
fn numbered(name: &str, base: &str) -> bool {
    if name == base {
        return true;
    }
    name.strip_prefix(base)
        .and_then(|rest| rest.strip_prefix(' '))
        .filter(|number| number.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|number| number.parse::<u32>().ok())
        .is_some_and(|number| number >= 2)
}

/// Whether `name` is a default the engine assigned for `launch`, including
/// the full registry name earlier engines used ("Claude Agent 2").
pub(crate) fn is_default_name(name: &str, launch: &HarnessLaunch) -> bool {
    name == LEGACY_DEFAULT_NAME
        || numbered(name, harness_label(launch))
        || numbered(name, launch.name.trim())
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
    use super::{harness_label, is_default_name};
    use soyeht_protocol::HarnessLaunch;

    fn launch(id: &str, name: &str) -> HarnessLaunch {
        HarnessLaunch {
            id: id.into(),
            name: name.into(),
            command: "agent".into(),
            args: vec![],
            version: "1".into(),
            environment: None,
        }
    }

    fn builtin(id: &str) -> HarnessLaunch {
        crate::agents::registry::builtin_entries()
            .unwrap()
            .into_iter()
            .find(|entry| entry.launch.id == id)
            .unwrap()
            .launch
    }

    #[test]
    fn panes_are_named_after_the_harness_not_its_adapter() {
        // The pinned registry names Claude's ACP adapter "Claude Agent".
        let claude = builtin("registry.claude-acp");
        assert_eq!(claude.name, "Claude Agent");
        assert_eq!(harness_label(&claude), "Claude");
        assert_eq!(harness_label(&builtin("registry.codex-acp")), "Codex");
        assert_eq!(harness_label(&launch("mine", "Agent")), "Agent");
        assert_eq!(harness_label(&launch("mine", "Test agent")), "Test agent");
        assert_eq!(harness_label(&launch("mine", "  ")), "mine");
    }

    #[test]
    fn default_names_are_the_harness_name_with_an_optional_ordinal() {
        let claude = launch("registry.claude-acp", "Claude Agent");
        assert!(is_default_name("Claude", &claude));
        assert!(is_default_name("Claude 2", &claude));
        assert!(is_default_name("Agent", &claude));
        // Names earlier engines derived from the full registry name.
        assert!(is_default_name("Claude Agent", &claude));
        assert!(is_default_name("Claude Agent 3", &claude));
        assert!(!is_default_name("Claude 1", &claude));
        assert!(!is_default_name("Claude 2b", &claude));
        assert!(!is_default_name("Claudette", &claude));
        assert!(!is_default_name("Reviewer", &claude));
    }
}
