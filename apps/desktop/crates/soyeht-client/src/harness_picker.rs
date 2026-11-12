//! How the agent pickers lay the engine's harnesses out, the rule of the Mac's
//! `HarnessPickerModel`: the ones installed on this machine come first, in groups by how ready
//! they are, and everything else sits under one collapsed "More harnesses" entry, so that a
//! catalogue of dozens of registry adapters does not bury the few that can run.

use soyeht_protocol::{HarnessInfo, HarnessSetupReason, HarnessSetupState, ModelInfo, ProbeResult};

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

/// The state a picker shows for `harness`. One the engine found on this machine and has not
/// started yet groups with the usable ones, as the setup checklist shows it (`found`), and its
/// label says "Installed" (`status_label`), not "Ready": "not checked" is the engine's word for a
/// session nobody has opened. The default choice prefers a session that opened (`setup_state`),
/// and takes the one harness found when there is only one.
fn shown_state(harness: &HarnessInfo) -> HarnessSetupState {
    match setup_state(harness) {
        HarnessSetupState::NotChecked if harness.available && harness.installed != Some(false) => {
            HarnessSetupState::Ready
        }
        state => state,
    }
}

/// How ready `harness` is: the group its row belongs to.
pub fn group_of(harness: &HarnessInfo) -> Group {
    match shown_state(harness) {
        HarnessSetupState::Ready => Group::Ready,
        HarnessSetupState::NotChecked => Group::NotChecked,
        HarnessSetupState::SignIn => Group::SignIn,
        // A state a later engine adds is a harness to look at.
        HarnessSetupState::CheckSetup | HarnessSetupState::Unknown => Group::CheckSetup,
        HarnessSetupState::NotInstalled => Group::NotInstalled,
    }
}

/// The harness a picker preselects: the one the person chose as their default when it can
/// start, else the ready one that opened a session most recently. The harness a pane already
/// runs (`excluding`) is never one.
pub fn default_harness<'a>(
    harnesses: &'a [HarnessInfo],
    excluding: Option<&str>,
    preferred: Option<&str>,
) -> Option<&'a HarnessInfo> {
    let candidates = || {
        harnesses
            .iter()
            .filter(|harness| harness.available && Some(harness.id.as_str()) != excluding)
    };
    if let Some(chosen) = preferred.and_then(|id| candidates().find(|harness| harness.id == id)) {
        return Some(chosen);
    }
    let last_ready = |harness: &HarnessInfo| {
        harness
            .last_ready_at_ms
            .as_deref()
            .map_or(0, |at| at.parse::<i64>().unwrap_or(0))
    };
    // A fresh machine with one harness found: the person has nothing to choose from.
    let mut found = candidates().filter(|harness| shown_state(harness) == HarnessSetupState::Ready);
    if let (Some(only), None) = (found.next(), found.next()) {
        return Some(only);
    }
    candidates()
        .filter(|harness| setup_state(harness) == HarnessSetupState::Ready)
        .min_by(|left, right| {
            last_ready(right)
                .cmp(&last_ready(left))
                .then_with(|| ordered(left, right))
        })
}

/// What the second line of a harness row, in short, says is wrong or right about it, in full:
/// the sentence the person reads when they open a harness's status.
pub fn status_reason(harness: &HarnessInfo) -> String {
    match (&harness.setup_reason, &harness.package) {
        (Some(HarnessSetupReason::RegistryPackageUnverified), Some(package)) => {
            download_notice(package, harness.package_locked == Some(true))
        }
        _ => fixed_reason(harness).to_owned(),
    }
}

/// What starting an adapter that a package runner fetches does, before the person starts it
/// (ADR 0143): the package it downloads and runs, and whether the engine installs it from a
/// lockfile it carries or the runner fetches it and its dependencies unchecked.
fn download_notice(package: &str, locked: bool) -> String {
    if locked {
        format!(
            "Starting it installs {package} once from a lockfile Soyeht carries, with every package checked, then runs it from there."
        )
    } else {
        format!(
            "Starting it downloads and runs {package} and its dependencies from the package registry. Soyeht has not checked them."
        )
    }
}

fn fixed_reason(harness: &HarnessInfo) -> &'static str {
    let reason = harness.setup_reason.clone().or(match setup_state(harness) {
        HarnessSetupState::Ready => None,
        HarnessSetupState::NotChecked => Some(HarnessSetupReason::NeverStarted),
        HarnessSetupState::SignIn => Some(HarnessSetupReason::AuthenticationRequired),
        HarnessSetupState::CheckSetup | HarnessSetupState::Unknown => {
            Some(HarnessSetupReason::LaunchFailed)
        }
        HarnessSetupState::NotInstalled => Some(HarnessSetupReason::ExecutableMissing),
    });
    match reason {
        Some(HarnessSetupReason::NeverStarted) => {
            "Installed; this agent has not been started yet. You can start it now."
        }
        Some(HarnessSetupReason::ExecutableMissing) => {
            "Executable not found on the engine's launch PATH."
        }
        Some(HarnessSetupReason::RegistryPackageUnverified) => {
            "Starting it downloads and runs this agent's adapter package, which Soyeht has not checked."
        }
        Some(HarnessSetupReason::AuthenticationRequired) => {
            "This agent requires sign-in before it can start."
        }
        Some(HarnessSetupReason::SessionFailed) => {
            "The agent started, but its session failed. Try starting it again."
        }
        Some(HarnessSetupReason::LaunchFailed) => {
            "The agent could not start. Check its installation and try again."
        }
        Some(HarnessSetupReason::EnvironmentUnavailable) => {
            "The engine could not read the launch environment."
        }
        Some(HarnessSetupReason::Unknown) => {
            "The engine reports a problem this version of Soyeht does not know. Update Soyeht."
        }
        None => "An agent session opened successfully.",
    }
}

/// The package a person acknowledges by starting `harness` from a picker that shows its notice
/// (ADR 0143): the one the runner downloads unchecked. `None` when the engine installs the
/// package from its lockfile, or the harness downloads none. The engine starts an unchecked
/// package only once a person sent this exact `name@version`.
pub fn acknowledged_package(harness: Option<&HarnessInfo>) -> Option<String> {
    harness
        .filter(|harness| harness.package_locked != Some(true))
        .and_then(|harness| harness.package.clone())
}

/// The line a harness picker shows above the model choices: why the harness needs attention, and
/// nothing for a ready one, whose "opened successfully" is no news while choosing a model.
pub fn picker_notice(harness: &HarnessInfo) -> Option<String> {
    (setup_state(harness) != HarnessSetupState::Ready).then(|| status_reason(harness))
}

/// A harness name without the ACP adapter's suffix ("Claude Agent" becomes "Claude"), the rule
/// the engine names agent panes by.
pub fn short_name(name: &str) -> &str {
    const SUFFIX: &str = " Agent";
    let trimmed = name.trim();
    match trimmed.strip_suffix(SUFFIX).map(str::trim) {
        Some(label) if !label.is_empty() => label,
        _ => trimmed,
    }
}

/// A last-resort name from a launch identifier: "registry.claude-acp" becomes "Claude". A raw
/// identifier is never shown.
pub fn readable_name(harness_id: &str) -> String {
    let mut name = harness_id;
    for prefix in ["registry.", "local.", "custom."] {
        name = name.strip_prefix(prefix).unwrap_or(name);
    }
    let name = name.strip_suffix("-acp").unwrap_or(name);
    let words: Vec<String> = name
        .split(['-', '_', '.'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut letters = word.chars();
            letters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(letters).collect()
            })
        })
        .collect();
    if words.is_empty() {
        "Agent".to_owned()
    } else {
        words.join(" ")
    }
}

/// What the harness chip of an agent composer says: the harness the pane runs, named by the
/// engine's catalogue ("Claude"), or the one it is switching to while a switch runs. Before the
/// catalogue loads, the running harness is called by the pane's own name and any other by a name
/// read from its identifier. The second value is whether a switch is running.
pub fn chip_title(
    harnesses: &[HarnessInfo],
    current: &str,
    switching_to: Option<&str>,
    pane_name: Option<&str>,
) -> (String, bool) {
    let shown = switching_to.unwrap_or(current);
    let title = match harnesses.iter().find(|harness| harness.id == shown) {
        Some(entry) => short_name(&entry.name).to_owned(),
        None => match pane_name.map(str::trim).filter(|name| !name.is_empty()) {
            Some(name) if shown == current => name.to_owned(),
            _ => readable_name(shown),
        },
    };
    (title, switching_to.is_some())
}

/// The button that hands a conversation over: it names the harness chosen.
pub fn switch_label(target: Option<&HarnessInfo>) -> String {
    match target {
        Some(target) => format!("Restart conversation with {}", target.name),
        None => "Choose another agent".to_owned(),
    }
}

/// A model's name in a list, saying so when the agent refused it before.
pub fn model_label(model: &ModelInfo) -> String {
    match model.observed_refusal {
        Some(_) => format!("{} \u{2014} refused", model.name),
        None => model.name.clone(),
    }
}

/// The second line of a harness row: how ready it is, which tells apart two harnesses that
/// share a name.
pub fn status_label(harness: &HarnessInfo) -> &'static str {
    // Found and never started is not "Ready": no session has opened yet.
    if setup_state(harness) == HarnessSetupState::NotChecked
        && shown_state(harness) == HarnessSetupState::Ready
    {
        return "Installed";
    }
    group_of(harness).title()
}

/// Whether "More harnesses" is open: when asked to be, when a search is running, when it is all
/// there is, and when the chosen harness lives there, as the Mac's picker opens it.
pub fn more_expanded(layout: &Layout, asked: bool, searching: bool, chosen: Option<&str>) -> bool {
    asked
        || searching
        || layout.sections.is_empty()
        || layout
            .more
            .iter()
            .any(|harness| Some(harness.id.as_str()) == chosen)
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

/// Whether `harness` answers to `query`: its name or its id contains it, whatever the case. An
/// empty query matches everything.
pub fn matches(harness: &HarnessInfo, query: &str) -> bool {
    let needle = query.trim().to_lowercase();
    needle.is_empty()
        || harness.name.to_lowercase().contains(&needle)
        || harness.id.to_lowercase().contains(&needle)
}

/// The harnesses of the picker for `query`, laid out. `excluding` is the harness a pane already
/// runs, which cannot be switched to.
pub fn layout<'a>(
    harnesses: &'a [HarnessInfo],
    query: &str,
    excluding: Option<&str>,
) -> Layout<'a> {
    let mut sections = Vec::new();
    let mut more = Vec::new();
    for wanted in GROUPS {
        let mut items: Vec<&HarnessInfo> = harnesses
            .iter()
            .filter(|harness| Some(harness.id.as_str()) != excluding)
            .filter(|harness| matches(harness, query))
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
mod tests;
