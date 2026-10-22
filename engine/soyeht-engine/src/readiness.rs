//! `engine.readiness`: which of the programs the engine and the harnesses need this machine has.
//!
//! The table of programs and the command that installs each on every system is `x-readiness`
//! in `contracts/protocol.json`. This file only asks the machine. A program is looked up where
//! the engine itself looks for it when it runs it, with no process started, so the answer is
//! the same as what a pane would meet. The lookups touch the file system (and the first one
//! can wait for the login shell's `PATH`), so they run off the Core lock (ADR 0079).

use crate::{
    agents::registry,
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
    offload::Offload,
};
use soyeht_platform::git::{Availability, Probe};
use soyeht_protocol::{
    EngineReadinessResult, ReadinessItem, ReadinessKind, ReadinessSystem,
    readiness::{HARNESSES, TOOLS, Tool, note_for},
};
use std::{path::Path, time::Duration};

/// The login shell that resolves the launch `PATH` is given five seconds, and the lookups that
/// follow are file-system stats.
const CHECK_TIMEOUT: Duration = Duration::from_secs(20);

impl Core {
    pub fn engine_readiness(&mut self) -> Result<Outcome> {
        let directory = self.directory.clone();
        Ok(Outcome::Offload(Offload::new(
            "Readiness check timed out",
            CHECK_TIMEOUT,
            move || report(&directory),
            |_, report| {
                serde_json::to_value(report?)
                    .map(Outcome::Reply)
                    .map_err(|_| errors::internal("Readiness serialization failed"))
            },
        )))
    }
}

fn report(directory: &Path) -> Result<EngineReadinessResult> {
    let system = system();
    let catalogue = registry::catalogue_entries(directory)?;
    let items = items(&system, tool_found, |id| {
        catalogue
            .iter()
            .find(|entry| entry.launch.id == id)
            .map(|entry| {
                (
                    entry.launch.name.clone(),
                    registry::locally_installed(entry, false),
                )
            })
    });
    Ok(EngineReadinessResult { system, items })
}

/// The tools that apply to `system`, then the harnesses the catalogue knows, each with the
/// install command for `system` while it is missing. `harness` answers a registry id with the
/// harness's name and whether its CLI is installed.
fn items(
    system: &ReadinessSystem,
    tool_found: impl Fn(&str) -> Option<bool>,
    harness: impl Fn(&str) -> Option<(String, bool)>,
) -> Vec<ReadinessItem> {
    let tools = TOOLS
        .iter()
        .filter(|tool| applies(tool, system))
        .filter_map(|tool| {
            let found = tool_found(tool.id)?;
            Some(item(
                tool.id,
                tool.name.into(),
                ReadinessKind::Tool,
                tool.required,
                found,
                tool.hints.for_system(system),
            ))
        });
    let harnesses = HARNESSES.iter().filter_map(|entry| {
        let (name, found) = harness(entry.id)?;
        Some(item(
            entry.id,
            name,
            ReadinessKind::Harness,
            false,
            found,
            entry.hints.for_system(system),
        ))
    });
    tools.chain(harnesses).collect()
}

fn item(
    id: &str,
    name: String,
    kind: ReadinessKind,
    required: bool,
    found: bool,
    hint: Option<&str>,
) -> ReadinessItem {
    ReadinessItem {
        id: id.into(),
        name,
        kind,
        required,
        found,
        hint: hint.filter(|_| !found).map(str::to_owned),
        note: hint
            .filter(|_| !found)
            .and_then(note_for)
            .map(str::to_owned),
    }
}

/// An unrecognised Linux runs what the other distributions run.
fn applies(tool: &Tool, system: &ReadinessSystem) -> bool {
    let system = if *system == ReadinessSystem::Other {
        &ReadinessSystem::Debian
    } else {
        system
    };
    tool.systems.contains(system)
}

/// Where each tool is looked up is where the engine looks when it runs it: `git` and `wt`
/// start from the engine's own `PATH`, `lsof` also from the standard places, and the harness
/// adapters through `npx` start from the login shell's `PATH`. `None` for a tool this file does
/// not know how to look up.
fn tool_found(id: &str) -> Option<bool> {
    Some(match id {
        "git" => {
            on_process_path("git") && Probe::system().availability() == Availability::Available
        }
        "node" => registry::executable("npx").is_some(),
        "wt" => on_process_path(soyeht_platform::worktrunk::PROGRAM),
        "lsof" => lsof_found(),
        _ => return None,
    })
}

fn on_process_path(command: &str) -> bool {
    let path = std::env::var_os("PATH").unwrap_or_default();
    registry::resolve_executable(command, std::env::split_paths(&path)).is_some()
}

#[cfg(unix)]
fn lsof_found() -> bool {
    crate::web::lsof_program().is_file()
}

/// Windows has no `lsof`; its tool table does not list it.
#[cfg(not(unix))]
fn lsof_found() -> bool {
    false
}

fn system() -> ReadinessSystem {
    if cfg!(target_os = "macos") {
        ReadinessSystem::Macos
    } else if cfg!(windows) {
        ReadinessSystem::Windows
    } else {
        // A system without the file is not one of the families we have commands for.
        linux_family(&std::fs::read_to_string("/etc/os-release").unwrap_or_default())
    }
}

/// The distribution family of an `os-release` file, from its `ID` and `ID_LIKE`.
fn linux_family(os_release: &str) -> ReadinessSystem {
    let words = os_release
        .lines()
        .filter_map(|line| {
            line.strip_prefix("ID=")
                .or_else(|| line.strip_prefix("ID_LIKE="))
        })
        .flat_map(|value| {
            value
                .trim_matches(|c| c == '"' || c == '\'')
                .split_whitespace()
        })
        .collect::<Vec<_>>();
    let is = |names: &[&str]| words.iter().any(|word| names.contains(word));
    if is(&["debian", "ubuntu"]) {
        ReadinessSystem::Debian
    } else if is(&["fedora", "rhel", "centos"]) {
        ReadinessSystem::Fedora
    } else {
        ReadinessSystem::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_tools(id: &str) -> Option<bool> {
        Some(id != "wt")
    }

    #[test]
    fn distributions_are_told_apart_by_their_id_and_the_ones_they_derive_from() {
        for (file, family) in [
            (
                "NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n",
                ReadinessSystem::Debian,
            ),
            (
                "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n",
                ReadinessSystem::Debian,
            ),
            ("ID=fedora\n", ReadinessSystem::Fedora),
            (
                "ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n",
                ReadinessSystem::Fedora,
            ),
            ("ID=arch\n", ReadinessSystem::Other),
            ("", ReadinessSystem::Other),
        ] {
            assert_eq!(linux_family(file), family, "{file}");
        }
    }

    #[test]
    fn a_found_program_has_no_hint_and_a_missing_one_has_its_systems_command() {
        let items = items(&ReadinessSystem::Fedora, all_tools, |id| {
            Some((id.into(), id == "registry.codex-acp"))
        });
        let wt = items.iter().find(|item| item.id == "wt").unwrap();
        assert!(!wt.found);
        assert_eq!(wt.hint.as_deref(), Some("brew install worktrunk"));
        assert_eq!(wt.note.as_deref(), Some("Needs Homebrew: https://brew.sh"));
        let git = items.iter().find(|item| item.id == "git").unwrap();
        assert!(git.found && git.required && git.hint.is_none());
        let claude = items
            .iter()
            .find(|item| item.id == "registry.claude-acp")
            .unwrap();
        assert!(!claude.found && !claude.required);
        assert_eq!(claude.kind, ReadinessKind::Harness);
        assert!(
            claude
                .hint
                .as_deref()
                .is_some_and(|hint| hint.starts_with("curl "))
        );
        assert!(claude.note.is_none(), "a vendor script needs no Homebrew");
        let codex = items
            .iter()
            .find(|item| item.id == "registry.codex-acp")
            .unwrap();
        assert!(codex.found && codex.hint.is_none());
    }

    #[test]
    fn a_tool_that_the_system_never_runs_is_not_listed() {
        let windows = items(&ReadinessSystem::Windows, all_tools, |_| None);
        assert!(windows.iter().all(|item| item.id != "lsof"));
        assert!(windows.iter().any(|item| item.id == "git"));
        let unknown = items(&ReadinessSystem::Other, all_tools, |_| None);
        let lsof = unknown.iter().find(|item| item.id == "lsof").unwrap();
        assert!(lsof.found, "an unknown Linux still runs lsof");
        let missing = items(&ReadinessSystem::Other, |_| Some(false), |_| None);
        assert!(
            missing.iter().all(|item| item.hint.is_none()),
            "no command for an unknown system"
        );
    }

    #[test]
    fn every_tool_of_the_table_can_be_looked_up_and_every_harness_is_in_the_catalogue() {
        for tool in TOOLS {
            assert!(tool_found(tool.id).is_some(), "{}", tool.id);
        }
        let catalogue = registry::builtin_entries().unwrap();
        for harness in HARNESSES {
            assert!(
                catalogue.iter().any(|entry| entry.launch.id == harness.id),
                "{}",
                harness.id
            );
        }
    }
}
