//! How the agent pickers lay the engine's harnesses out, the rule of the Mac's
//! `HarnessPickerModel`: the ones installed on this machine come first, in groups by how ready
//! they are, and everything else sits under one collapsed "More harnesses" entry, so that a
//! catalogue of dozens of registry adapters does not bury the few that can run.

use soyeht_protocol::{HarnessInfo, HarnessSetupState, ProbeResult};

/// The title of the collapsed entry.
pub const MORE_TITLE: &str = "More harnesses";

/// How ready an installed harness is; the order here is the order of the groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Ready,
    NotChecked,
    SignIn,
    CheckSetup,
    NotInstalled,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::NotChecked => "Not checked",
            Self::SignIn => "Sign in",
            Self::CheckSetup => "Check setup",
            Self::NotInstalled => "Not installed",
        }
    }
}

/// The harnesses to show, split for display. Each harness appears exactly once, and the order
/// is a function of the catalogue alone, so it does not shift between refreshes.
#[derive(Debug, PartialEq)]
pub struct Layout<'a> {
    pub sections: Vec<(Group, Vec<&'a HarnessInfo>)>,
    /// The harnesses that are not installed, in group order and then by name.
    pub more: Vec<&'a HarnessInfo>,
}

/// The engine's own word for the setup of `harness`, or the one its other fields imply.
fn setup_state(harness: &HarnessInfo) -> HarnessSetupState {
    if let Some(state) = &harness.setup_state {
        return state.clone();
    }
    if !harness.available {
        return HarnessSetupState::NotInstalled;
    }
    if harness
        .capabilities
        .iter()
        .any(|capability| capability.result == ProbeResult::Unauthenticated)
    {
        return HarnessSetupState::SignIn;
    }
    HarnessSetupState::NotChecked
}

/// How ready `harness` is: the group its row belongs to.
pub fn group_of(harness: &HarnessInfo) -> Group {
    match setup_state(harness) {
        HarnessSetupState::Ready => Group::Ready,
        HarnessSetupState::NotChecked => Group::NotChecked,
        HarnessSetupState::SignIn => Group::SignIn,
        HarnessSetupState::CheckSetup => Group::CheckSetup,
        HarnessSetupState::NotInstalled => Group::NotInstalled,
    }
}

/// The second line of a harness row: how ready it is, which tells apart two harnesses that
/// share a name.
pub fn status_label(harness: &HarnessInfo) -> &'static str {
    group_of(harness).title()
}

/// Whether "More harnesses" is open: when asked to be, and also when it is all there is, as
/// the Mac's picker opens it when no installed harness is left to show. The desktop menus pick
/// on click and keep no selection, so a selection that lives under "More" needs no rule here.
pub fn more_expanded(layout: &Layout, asked: bool) -> bool {
    asked || layout.sections.is_empty()
}

/// Whether the harness is on this machine: it has opened a session, or its executable resolves
/// and the engine did not say it is reachable only through an unverified package runner.
pub fn is_installed(harness: &HarnessInfo) -> bool {
    setup_state(harness) == HarnessSetupState::Ready
        || (harness.available && harness.installed != Some(false))
}

fn ordered(left: &HarnessInfo, right: &HarnessInfo) -> std::cmp::Ordering {
    left.name
        .to_lowercase()
        .cmp(&right.name.to_lowercase())
        .then_with(|| left.id.cmp(&right.id))
}

const GROUPS: [Group; 5] = [
    Group::Ready,
    Group::NotChecked,
    Group::SignIn,
    Group::CheckSetup,
    Group::NotInstalled,
];

pub fn layout(harnesses: &[HarnessInfo]) -> Layout<'_> {
    let mut sections = Vec::new();
    let mut more = Vec::new();
    for wanted in GROUPS {
        let mut items: Vec<&HarnessInfo> = harnesses
            .iter()
            .filter(|harness| group_of(harness) == wanted)
            .collect();
        items.sort_by(|left, right| ordered(left, right));
        let (installed, other): (Vec<_>, Vec<_>) =
            items.into_iter().partition(|harness| is_installed(harness));
        more.extend(other);
        if !installed.is_empty() {
            sections.push((wanted, installed));
        }
    }
    Layout { sections, more }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn harness(id: &str, name: &str, available: bool, installed: Option<bool>) -> HarnessInfo {
        let mut value = json!({
            "id": id, "name": name, "available": available,
            "capabilities": [], "options": [], "authMethods": [],
        });
        if let Some(installed) = installed {
            value["installed"] = json!(installed);
        }
        serde_json::from_value(value).unwrap()
    }

    fn with_state(mut harness: HarnessInfo, state: HarnessSetupState) -> HarnessInfo {
        harness.setup_state = Some(state);
        harness
    }

    fn ids<'a>(items: &[&'a HarnessInfo]) -> Vec<&'a str> {
        items.iter().map(|harness| harness.id.as_str()).collect()
    }

    #[test]
    fn installed_harnesses_come_first_in_groups_and_a_package_runner_waits_under_more() {
        let catalogue = [
            harness("registry.cline", "Cline", true, Some(false)),
            with_state(
                harness("codex", "Codex", true, None),
                HarnessSetupState::Ready,
            ),
            harness("claude", "claude", true, Some(true)),
            with_state(
                harness("gemini", "Gemini", true, None),
                HarnessSetupState::SignIn,
            ),
            harness("registry.auggie", "Auggie CLI", false, None),
        ];
        let layout = layout(&catalogue);
        let groups: Vec<_> = layout
            .sections
            .iter()
            .map(|(group, items)| (group.title(), ids(items)))
            .collect();
        assert_eq!(
            groups,
            [
                ("Ready", vec!["codex"]),
                ("Not checked", vec!["claude"]),
                ("Sign in", vec!["gemini"]),
            ]
        );
        assert_eq!(ids(&layout.more), ["registry.cline", "registry.auggie"]);
    }

    #[test]
    fn a_harness_that_ran_before_is_installed_even_when_a_runner_reaches_it() {
        let ran = with_state(
            harness("registry.codex-acp", "Codex", true, Some(false)),
            HarnessSetupState::Ready,
        );
        assert!(is_installed(&ran));
        assert!(!is_installed(&harness("a", "A", true, Some(false))));
        assert!(!is_installed(&harness("b", "B", false, None)));
    }

    #[test]
    fn names_sort_without_regard_to_case_then_by_id() {
        let catalogue = [
            harness("b", "beta", true, Some(true)),
            harness("a2", "Alpha", true, Some(true)),
            harness("a1", "alpha", true, Some(true)),
        ];
        let layout = layout(&catalogue);
        assert_eq!(ids(&layout.sections[0].1), ["a1", "a2", "b"]);
    }

    #[test]
    fn the_second_line_says_how_ready_a_harness_is() {
        let ready = with_state(harness("a", "Claude", true, None), HarnessSetupState::Ready);
        let unchecked = harness("b", "Claude", true, Some(true));
        let missing = harness("c", "Claude", false, None);
        assert_eq!(status_label(&ready), "Ready");
        assert_eq!(status_label(&unchecked), "Not checked");
        assert_eq!(status_label(&missing), "Not installed");
    }

    #[test]
    fn more_opens_when_asked_or_when_nothing_installed_is_left() {
        let installed = [
            harness("a", "A", true, Some(true)),
            harness("b", "B", false, None),
        ];
        let with_installed = layout(&installed);
        assert!(!more_expanded(&with_installed, false));
        assert!(more_expanded(&with_installed, true));
        let none_installed = [harness("b", "B", false, None)];
        let only_more = layout(&none_installed);
        assert!(more_expanded(&only_more, false));
    }

    #[test]
    fn an_empty_catalogue_has_nothing_to_show() {
        let layout = layout(&[]);
        assert!(layout.sections.is_empty() && layout.more.is_empty());
    }
}
