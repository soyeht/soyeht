use super::{package_runner, registry, responses, state::Phase};
use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
    streak::KeyedStreaks,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soyeht_platform::log::{self, Failure as _, Line};
use soyeht_protocol::{
    HarnessEnvironmentIssue, HarnessInfo, HarnessSetupReason, HarnessSetupState, HarnessSource,
    ModelInfo, ModelListSource, ModelRefusal, limits, method,
};
use soyeht_store::agents::AgentModelRefusal;
use soyeht_store::agents::HarnessReadiness;
use std::time::Instant;
use std::{collections::BTreeSet, path::PathBuf};

fn wire_environment_issue(issue: super::environment::LaunchPathIssue) -> HarnessEnvironmentIssue {
    use super::environment::LaunchPathIssue;
    match issue {
        LaunchPathIssue::MissingShell => HarnessEnvironmentIssue::MissingShell,
        LaunchPathIssue::UnsupportedShell => HarnessEnvironmentIssue::UnsupportedShell,
        LaunchPathIssue::UnavailableShell => HarnessEnvironmentIssue::UnavailableShell,
        LaunchPathIssue::ProbeFailed => HarnessEnvironmentIssue::ProbeFailed,
        LaunchPathIssue::ProbeTimedOut => HarnessEnvironmentIssue::ProbeTimedOut,
        LaunchPathIssue::OutputTooLarge => HarnessEnvironmentIssue::OutputTooLarge,
        LaunchPathIssue::InvalidOutput => HarnessEnvironmentIssue::InvalidOutput,
    }
}

fn environment_issue(rescan: bool) -> Option<HarnessEnvironmentIssue> {
    let resolution = if rescan {
        super::environment::refresh_launch_path()
    } else {
        super::environment::launch_path_resolution()
    };
    resolution.issue.map(wire_environment_issue)
}

/// What the engine knows about the package a harness's launch downloads, for [`setup_state`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Download {
    /// No package runner: a program on the `PATH`, or a custom entry.
    None,
    /// A package runner, and the person has not acknowledged what the first start downloads:
    /// the engine installs it from its lock (and says so), or the runner fetches it unchecked.
    Unacknowledged,
    /// The person acknowledged the exact package.
    Acknowledged,
}

fn setup_state(
    resolved: bool,
    installed: bool,
    download: Download,
    environment_issue: bool,
    recorded: Option<&HarnessReadiness>,
    phase: Option<&Phase>,
) -> (HarnessSetupState, Option<HarnessSetupReason>) {
    if !resolved {
        return (
            HarnessSetupState::NotInstalled,
            Some(if environment_issue {
                HarnessSetupReason::EnvironmentUnavailable
            } else {
                HarnessSetupReason::ExecutableMissing
            }),
        );
    }
    // What the notice is for: a package nobody started yet. A program of the same name on the
    // `PATH` does not end it (the runner still fetches the package), and neither does the record
    // of a start that never got going (`launch_failed`); only a record of a process that ran does.
    let ran = recorded.is_some_and(|value| value.reason != Some(HarnessSetupReason::LaunchFailed));
    if download == Download::Unacknowledged && !ran {
        return (
            HarnessSetupState::NotChecked,
            Some(HarnessSetupReason::RegistryPackageUnverified),
        );
    }
    if let Some(recorded) = recorded {
        return (recorded.state.clone(), recorded.reason.clone());
    }
    if !installed {
        return if download == Download::None {
            (
                HarnessSetupState::NotInstalled,
                Some(HarnessSetupReason::ExecutableMissing),
            )
        } else {
            (
                HarnessSetupState::NotChecked,
                Some(HarnessSetupReason::NeverStarted),
            )
        };
    }
    match phase {
        Some(Phase::Ready) => (HarnessSetupState::Ready, None),
        Some(Phase::NeedsLogin) => (
            HarnessSetupState::SignIn,
            Some(HarnessSetupReason::AuthenticationRequired),
        ),
        Some(Phase::Faulted) => (
            HarnessSetupState::CheckSetup,
            Some(HarnessSetupReason::SessionFailed),
        ),
        _ => (
            HarnessSetupState::NotChecked,
            Some(HarnessSetupReason::NeverStarted),
        ),
    }
}

fn page(items: Vec<Value>, mut fixed: Value, params: &Value) -> Result<Value> {
    let digest = format!(
        "{:x}",
        Sha256::digest(errors::json_bytes(&json!([items, fixed]))?)
    );
    let start = if let Some(cursor) = params["cursor"].as_str() {
        let (prior, index) = cursor
            .split_once('.')
            .ok_or_else(|| errors::invalid("Invalid catalogue cursor"))?;
        if prior != digest {
            return Err(errors::resync_required(
                "Catalogue changed; reload the first page",
            ));
        }
        index
            .parse::<usize>()
            .ok()
            .filter(|index| *index < items.len())
            .ok_or_else(|| errors::invalid("Invalid catalogue position"))?
    } else {
        0
    };
    let mut accepted = Vec::new();
    let mut bytes = errors::json_bytes(&fixed)?.len() + 512;
    let maximum = params["limit"].as_u64().unwrap_or(64) as usize;
    for item in items.iter().skip(start).take(maximum) {
        let size = errors::json_bytes(item)?.len() + 1;
        if bytes + size > limits::JSON_BYTES {
            if accepted.is_empty() {
                return Err(errors::invalid("Catalogue entry exceeds frame capacity"));
            }
            break;
        }
        accepted.push(item.clone());
        bytes += size;
    }
    if start + accepted.len() < items.len() {
        fixed["nextCursor"] = json!(format!("{digest}.{}", start + accepted.len()));
    }
    fixed["items"] = json!(accepted);
    Ok(fixed)
}

fn model_items(
    options: &[soyeht_protocol::ConfigOption],
    refusals: &[AgentModelRefusal],
) -> Vec<Value> {
    options
        .iter()
        .find(|option| option.category.as_deref() == Some("model") || option.id == "model")
        .map(|option| {
            option
                .options
                .iter()
                .map(|choice| {
                    let observed_refusal = refusals
                        .iter()
                        .find(|refusal| refusal.model_id == choice.value)
                        .map(|refusal| ModelRefusal {
                            reason: refusal.reason.clone(),
                            observed_at: refusal.observed_at.clone(),
                        });
                    json!(ModelInfo {
                        id: choice.value.clone(),
                        name: choice.name.clone(),
                        description: choice.description.clone(),
                        observed_refusal,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) struct PreparedHarness {
    entry: registry::RegistryEntry,
    resolved: Option<soyeht_protocol::HarnessLaunch>,
    prepared: Option<(soyeht_protocol::HarnessLaunch, String)>,
    installed_without_success: bool,
    installed_with_success: bool,
}

pub(crate) enum PreparedCatalog {
    Harness {
        entries: Vec<PreparedHarness>,
        issue: Option<HarnessEnvironmentIssue>,
    },
    Model {
        harness: String,
        prepared: Option<(soyeht_protocol::HarnessLaunch, String)>,
    },
}

/// The launch of a harness that has a live session and the digest of its executable. A harness
/// that cannot be prepared is listed as unprepared, and the reason is logged by kind.
fn prepare_or_log(
    entry: &registry::RegistryEntry,
) -> Option<(soyeht_protocol::HarnessLaunch, String)> {
    // Every `harness.list` prepares again, so a harness whose executable is gone would log on
    // each one: the streak of each harness logs its first failure and each change of kind.
    static FAILED: KeyedStreaks = KeyedStreaks::new();
    let id = &entry.launch.id;
    match registry::prepare_entry(entry.clone()) {
        Ok(prepared) => {
            FAILED.succeed(id);
            Some(prepared)
        }
        Err(error) => {
            if FAILED.fail(id, &error.failure_kind()) {
                log::failure("engine.harness_prepare_failed", &error);
            }
            None
        }
    }
}

pub(crate) fn prepare_catalog(
    directory: PathBuf,
    method: &str,
    params: &Value,
    session_harnesses: BTreeSet<String>,
) -> Result<PreparedCatalog> {
    if method == method::HARNESS_LIST {
        let started = Instant::now();
        let rescan = params["rescan"].as_bool() == Some(true) && params["cursor"].is_null();
        let issue = environment_issue(rescan);
        let environment_ms = started.elapsed().as_millis();
        let mut resolve_ms = 0;
        let mut prepare_ms = 0;
        let mut installed_ms = 0;
        let entries = registry::listed_entries(&directory)?
            .into_iter()
            .map(|entry| {
                let step = Instant::now();
                let resolved = registry::resolve_entry(entry.clone());
                resolve_ms += step.elapsed().as_millis();
                let step = Instant::now();
                let prepared = session_harnesses
                    .contains(&entry.launch.id)
                    .then(|| prepare_or_log(&entry))
                    .flatten();
                prepare_ms += step.elapsed().as_millis();
                let step = Instant::now();
                let installed_without_success = registry::locally_installed(&entry, false);
                let installed_with_success = registry::locally_installed(&entry, true);
                installed_ms += step.elapsed().as_millis();
                PreparedHarness {
                    entry,
                    resolved,
                    prepared,
                    installed_without_success,
                    installed_with_success,
                }
            })
            .collect::<Vec<_>>();
        Line::new("engine.catalog_prepared")
            .field("method", method::HARNESS_LIST)
            .field("entries", entries.len())
            .field("environmentMs", environment_ms)
            .field("resolveMs", resolve_ms)
            .field("prepareMs", prepare_ms)
            .field("installedMs", installed_ms)
            .field("totalMs", started.elapsed().as_millis())
            .emit();
        return Ok(PreparedCatalog::Harness { entries, issue });
    }
    let started = Instant::now();
    let harness = errors::required(params["harness"].as_str(), "Harness ID is required")?;
    let entries = registry::entries(&directory)?;
    if !entries.iter().any(|entry| entry.id == harness) {
        return Err(errors::harness_unavailable("Harness is not configured"));
    }
    let prepared = if params["paneId"].as_str().is_some() {
        None
    } else if session_harnesses.contains(harness) {
        Some(registry::prepare(&directory, harness)?)
    } else {
        // Only a live session can have probed models, so with none there is
        // nothing to match and no reason to read the executable to hash it.
        registry::resolve(&directory, harness)?;
        None
    };
    Line::new("engine.catalog_prepared")
        .field("method", method::MODEL_LIST)
        .field("totalMs", started.elapsed().as_millis())
        .emit();
    Ok(PreparedCatalog::Model {
        harness: harness.to_owned(),
        prepared,
    })
}

fn harness_info(core: &Core, item: PreparedHarness, issue: bool) -> Result<Value> {
    let PreparedHarness {
        entry,
        resolved,
        prepared,
        installed_without_success,
        installed_with_success,
    } = item;
    let known = prepared.as_ref().and_then(|(launch, digest)| {
        core.agents
            .values()
            .filter(|agent| agent.state.launch == *launch && agent.state.binary_digest == *digest)
            .max_by_key(|agent| agent.state.last_update_ms)
    });
    let mut capabilities = Vec::new();
    if let Some(known) = known {
        for method in responses::PROBED_METHODS {
            if let Some(capability) = core
                .store
                .agent_probe(&known.state.fingerprint, method)
                .map_err(errors::storage)?
            {
                capabilities.push(capability);
            }
        }
    }
    let initialized = known.and_then(|agent| agent.state.initialized.as_ref());
    let recorded = core
        .store
        .harness_readiness(&entry.launch.id)
        .map_err(errors::storage)?;
    let installed = if recorded
        .as_ref()
        .and_then(|value| value.last_ready_ms)
        .is_some()
        || known.is_some_and(|agent| agent.state.phase == Phase::Ready)
    {
        installed_with_success
    } else {
        installed_without_success
    };
    let package_runner = entry.source == HarnessSource::BuiltIn
        && package_runner::package_runner(&entry.launch.command).is_some();
    let package = super::adapter_install::runner_package(&entry.launch);
    let acknowledged = match super::adapter_consent::unchecked_package(&entry.launch) {
        Some(package) => soyeht_store::adapter_acknowledgement::acknowledged(&core.store, &package)
            .map_err(errors::storage)?,
        None => false,
    };
    let download = match (package_runner, acknowledged) {
        (false, _) => Download::None,
        (true, true) => Download::Acknowledged,
        (true, false) => Download::Unacknowledged,
    };
    let (setup_state, setup_reason) = setup_state(
        resolved.is_some(),
        installed,
        download,
        issue,
        recorded.as_ref(),
        known.map(|agent| &agent.state.phase),
    );
    Ok(json!(HarnessInfo {
        // Pickers show the same label panes are named after ("Claude", not the
        // registry's "Claude Agent"), so clients need no naming rule of their own.
        name: super::naming::harness_label(&entry.launch).to_owned(),
        id: entry.launch.id,
        available: resolved.is_some(),
        installed: Some(installed),
        version: initialized.map(|i| i.version.clone()).or_else(|| (!entry
            .launch
            .version
            .is_empty())
        .then(|| entry.launch.version.clone())),
        fingerprint: known.map(|agent| agent.state.fingerprint.clone()),
        capabilities,
        options: known
            .map(|agent| agent.state.options.values.clone())
            .unwrap_or_default(),
        auth_methods: initialized
            .map(|i| i.auth_methods.iter().map(|m| m.public.clone()).collect())
            .unwrap_or_default(),
        source: Some(entry.source),
        setup_state: Some(setup_state),
        setup_reason,
        last_ready_at_ms: recorded.and_then(|value| value.last_ready_ms.map(|ms| ms.to_string())),
        package_locked: package.as_ref().map(|(_, locked)| *locked),
        package: package.map(|(name, _)| name),
    }))
}

impl Core {
    pub(crate) fn agent_catalog_prepared(
        &self,
        prepared: PreparedCatalog,
        params: &Value,
    ) -> Result<Value> {
        if let PreparedCatalog::Harness { entries, issue } = prepared {
            let mut items = Vec::new();
            for entry in entries {
                items.push(harness_info(self, entry, issue.is_some())?);
            }
            // An optional field is left out when it has no value, never sent as null.
            let fixed = issue.map_or_else(|| json!({}), |issue| json!({"environmentIssue": issue}));
            return page(items, fixed, params);
        }
        let PreparedCatalog::Model { harness, prepared } = prepared else {
            return Err(errors::internal("Wrong catalogue preparation"));
        };
        let known = if let Some(pane) = params["paneId"].as_str() {
            let state = self
                .agents
                .get(pane)
                .filter(|a| {
                    a.state.launch.id == harness
                        && self.panes.get(pane).is_some_and(|p| !p.stored.closed)
                })
                .ok_or_else(|| errors::not_found("Agent pane not found for this harness"))?;
            Some(state)
        } else {
            prepared.and_then(|(launch, digest)| {
                self.agents
                    .values()
                    .filter(|a| a.state.launch == launch && a.state.binary_digest == digest)
                    .max_by_key(|a| a.state.last_update_ms)
            })
        };
        let options = known
            .map(|a| a.state.options.values.clone())
            .unwrap_or_default();
        let refusals = self
            .store
            .agent_model_refusals(&harness)
            .map_err(errors::storage)?;
        let items = model_items(&options, &refusals);
        page(
            items,
            json!({"options":options,"source":if known.is_some() {ModelListSource::NativeSession} else {ModelListSource::NotProbed}}),
            params,
        )
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn setup_state_uses_observed_launch_facts_without_mistaking_missing_probes_for_failure() {
        assert_eq!(
            setup_state(true, true, Download::None, false, None, None),
            (
                HarnessSetupState::NotChecked,
                Some(HarnessSetupReason::NeverStarted)
            )
        );
        let ready = HarnessReadiness {
            state: HarnessSetupState::Ready,
            reason: None,
            last_ready_ms: Some(42),
        };
        assert_eq!(
            setup_state(true, true, Download::None, false, Some(&ready), None),
            (HarnessSetupState::Ready, None)
        );
        assert_eq!(
            setup_state(
                true,
                true,
                Download::None,
                false,
                None,
                Some(&Phase::NeedsLogin)
            ),
            (
                HarnessSetupState::SignIn,
                Some(HarnessSetupReason::AuthenticationRequired)
            )
        );
        assert_eq!(
            setup_state(false, true, Download::None, false, Some(&ready), None),
            (
                HarnessSetupState::NotInstalled,
                Some(HarnessSetupReason::ExecutableMissing)
            )
        );
        assert_eq!(
            setup_state(false, true, Download::None, true, Some(&ready), None),
            (
                HarnessSetupState::NotInstalled,
                Some(HarnessSetupReason::EnvironmentUnavailable)
            )
        );
        assert_eq!(
            setup_state(true, false, Download::Unacknowledged, false, None, None),
            (
                HarnessSetupState::NotChecked,
                Some(HarnessSetupReason::RegistryPackageUnverified)
            )
        );
        let login = HarnessReadiness {
            state: HarnessSetupState::SignIn,
            reason: Some(HarnessSetupReason::AuthenticationRequired),
            last_ready_ms: None,
        };
        assert_eq!(
            setup_state(
                true,
                false,
                Download::Unacknowledged,
                false,
                Some(&login),
                None
            ),
            (
                HarnessSetupState::SignIn,
                Some(HarnessSetupReason::AuthenticationRequired)
            )
        );
    }

    /// The notice about what the first start downloads is not replaced by the record of a start
    /// that failed, and ends when the person acknowledged the package.
    #[test]
    fn the_download_notice_outlives_a_failed_first_start_and_ends_with_the_acknowledgement() {
        let failed = HarnessReadiness {
            state: HarnessSetupState::CheckSetup,
            reason: Some(HarnessSetupReason::LaunchFailed),
            last_ready_ms: None,
        };
        let notice = (
            HarnessSetupState::NotChecked,
            Some(HarnessSetupReason::RegistryPackageUnverified),
        );
        assert_eq!(
            setup_state(
                true,
                false,
                Download::Unacknowledged,
                false,
                Some(&failed),
                None
            ),
            notice
        );
        // A program of the same name on the `PATH` does not end it: the runner fetches the package.
        assert_eq!(
            setup_state(true, true, Download::Unacknowledged, false, None, None),
            notice
        );
        // Once the person acknowledged it, the failure is what is shown.
        assert_eq!(
            setup_state(
                true,
                false,
                Download::Acknowledged,
                false,
                Some(&failed),
                None
            ),
            (
                HarnessSetupState::CheckSetup,
                Some(HarnessSetupReason::LaunchFailed)
            )
        );
        assert_eq!(
            setup_state(true, false, Download::Acknowledged, false, None, None),
            (
                HarnessSetupState::NotChecked,
                Some(HarnessSetupReason::NeverStarted)
            )
        );
    }

    #[test]
    fn every_launch_path_issue_has_a_wire_value() {
        use super::super::environment::LaunchPathIssue;
        let pairs = [
            (
                LaunchPathIssue::MissingShell,
                HarnessEnvironmentIssue::MissingShell,
            ),
            (
                LaunchPathIssue::UnsupportedShell,
                HarnessEnvironmentIssue::UnsupportedShell,
            ),
            (
                LaunchPathIssue::UnavailableShell,
                HarnessEnvironmentIssue::UnavailableShell,
            ),
            (
                LaunchPathIssue::ProbeFailed,
                HarnessEnvironmentIssue::ProbeFailed,
            ),
            (
                LaunchPathIssue::ProbeTimedOut,
                HarnessEnvironmentIssue::ProbeTimedOut,
            ),
            (
                LaunchPathIssue::OutputTooLarge,
                HarnessEnvironmentIssue::OutputTooLarge,
            ),
            (
                LaunchPathIssue::InvalidOutput,
                HarnessEnvironmentIssue::InvalidOutput,
            ),
        ];
        for (issue, expected) in pairs {
            assert_eq!(wire_environment_issue(issue), expected);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::ErrorKind;
    #[test]
    fn catalogue_pages_preserve_large_entries_and_reject_changed_cursors() {
        let items = (0..5)
            .map(|n| json!({"id":n,"text":"x".repeat(400_000)}))
            .collect::<Vec<_>>();
        let first = page(items.clone(), json!({}), &json!({})).unwrap();
        assert!(serde_json::to_vec(&first).unwrap().len() <= limits::JSON_BYTES);
        assert_eq!(first["items"].as_array().unwrap().len(), 2);
        let second = page(
            items.clone(),
            json!({}),
            &json!({"cursor":first["nextCursor"]}),
        )
        .unwrap();
        assert_eq!(second["items"][0]["id"], 2);
        let mut changed = items;
        changed[0]["text"] = json!("changed");
        assert_eq!(
            errors::kind(
                &page(changed, json!({}), &json!({"cursor":first["nextCursor"]})).unwrap_err(),
            ),
            ErrorKind::ResyncRequired
        );
    }

    #[cfg(unix)]
    #[test]
    fn model_list_reads_an_executable_only_for_a_harness_with_a_live_session() {
        use std::os::unix::fs::PermissionsExt;
        let directory =
            std::env::temp_dir().join(format!("soyeht-model-list-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        // Executable but unreadable: resolving it works, hashing it cannot.
        let executable = directory.join("agent");
        std::fs::write(&executable, b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o111)).unwrap();
        let launch = soyeht_protocol::HarnessLaunch {
            id: "slow-disk".into(),
            name: "Slow disk".into(),
            command: executable.to_str().unwrap().into(),
            args: vec![],
            version: "1".into(),
            environment: None,
        };
        registry::create_custom(&directory, launch).unwrap();
        let params = json!({"harness":"slow-disk"});

        let idle = prepare_catalog(directory.clone(), "model.list", &params, BTreeSet::new());
        assert!(matches!(
            idle.unwrap(),
            PreparedCatalog::Model { prepared: None, .. }
        ));

        let with_session = BTreeSet::from(["slow-disk".to_owned()]);
        let can_read = std::fs::File::open(&executable).is_ok();
        let live = prepare_catalog(directory.clone(), "model.list", &params, with_session);
        if can_read {
            // Privileged runs (root) read a mode 0o111 file, so the unreadable-file
            // failure cannot be produced here. Say so instead of passing silently.
            eprintln!("skipped: this user reads mode 0o111 files, so hashing cannot fail");
            assert!(live.is_ok(), "a readable executable is hashed");
        } else {
            assert!(live.is_err(), "an unreadable executable cannot be hashed");
        }

        let unknown = prepare_catalog(
            directory.clone(),
            "model.list",
            &json!({"harness":"nope"}),
            BTreeSet::new(),
        );
        assert!(unknown.is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn model_list_for_an_idle_harness_is_not_probed_and_empty() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-model-idle-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let core = Core::open(&directory).unwrap();
        let params = json!({"harness":"registry.claude-acp"});
        let prepared = PreparedCatalog::Model {
            harness: "registry.claude-acp".into(),
            prepared: None,
        };
        let listed = core.agent_catalog_prepared(prepared, &params).unwrap();
        assert_eq!(listed["source"], "not_probed");
        assert_eq!(listed["items"], json!([]));
        drop(core);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn harness_list_names_harnesses_with_the_label_panes_are_named_after() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-harness-label-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let core = Core::open(&directory).unwrap();
        let prepared = prepare_catalog(
            directory.clone(),
            "harness.list",
            &json!({}),
            BTreeSet::new(),
        )
        .unwrap();
        let listed = core.agent_catalog_prepared(prepared, &json!({})).unwrap();
        let entries = registry::catalogue_entries(&directory).unwrap();
        let claude = entries
            .iter()
            .find(|entry| entry.launch.id == "registry.claude-acp")
            .unwrap();
        let item = listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == "registry.claude-acp")
            .unwrap();
        // The registry calls the adapter "Claude Agent"; panes and pickers say "Claude".
        assert_eq!(claude.launch.name, "Claude Agent");
        assert_eq!(
            item["name"],
            super::super::naming::harness_label(&claude.launch)
        );
        assert_eq!(item["name"], "Claude");
        drop(core);
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// `harness.list` lists only the checked harnesses, so only a test reaches the catalogue
    /// item of one it does not check: the package its first start downloads unchecked, and the
    /// stored acknowledgement that ends the notice about it.
    #[test]
    fn harness_info_shows_an_unacknowledged_registry_package_and_the_acknowledgement_ends_it() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-harness-package-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let mut core = Core::open(&directory).unwrap();
        let entry = registry::builtin_entries()
            .unwrap()
            .into_iter()
            .find(|entry| entry.launch.id == "registry.gemini")
            .unwrap();
        let (package, locked) =
            super::super::adapter_install::runner_package(&entry.launch).unwrap();
        assert!(!locked);
        // `prepare_catalog` resolves the command on this host's `PATH`, which only decides
        // "available"; the resolved launch is given directly so that the notice under test is
        // exercised on a host without the package runner too.
        let prepared = |entry: &registry::RegistryEntry| PreparedHarness {
            entry: entry.clone(),
            resolved: Some(entry.launch.clone()),
            prepared: None,
            installed_without_success: registry::locally_installed(entry, false),
            installed_with_success: registry::locally_installed(entry, true),
        };

        let before = harness_info(&core, prepared(&entry), false).unwrap();
        assert_eq!(before["package"], json!(&package));
        assert_eq!(before["packageLocked"], json!(false));
        assert_eq!(
            before["setupReason"],
            json!(HarnessSetupReason::RegistryPackageUnverified)
        );

        crate::ports::StorePort::mutate(&mut core.store, "test", &package, &package, |change| {
            soyeht_store::adapter_acknowledgement::acknowledge(change, &package, 1)?;
            Ok((json!({}), Vec::new()))
        })
        .unwrap();

        let after = harness_info(&core, prepared(&entry), false).unwrap();
        assert_eq!(after["package"], json!(&package));
        assert_eq!(after["packageLocked"], json!(false));
        // With the exact package acknowledged, the unacknowledged download no longer is what
        // the item reports: nothing was recorded about this harness yet, so it is only unchecked.
        assert_eq!(
            after["setupReason"],
            json!(HarnessSetupReason::NeverStarted)
        );
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn only_the_four_checked_harnesses_are_supported_and_they_are_the_readiness_table() {
        let supported = registry::builtin_entries()
            .unwrap()
            .into_iter()
            .filter(|entry| entry.support == registry::Support::Supported)
            .map(|entry| entry.launch.id)
            .collect::<BTreeSet<_>>();
        let readiness = soyeht_protocol::readiness::HARNESSES
            .iter()
            .map(|harness| harness.id.to_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(supported, readiness);
        assert_eq!(supported.len(), 4);
    }

    #[test]
    fn an_unchecked_harness_is_not_listed_for_a_new_pane_but_still_resolves_for_a_bound_one() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-harness-support-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(
            directory.join("harnesses.json"),
            br#"[{"id":"mine","name":"Mine","command":"mine","args":[],"version":"1"}]"#,
        )
        .unwrap();
        let listed = registry::listed_entries(&directory)
            .unwrap()
            .into_iter()
            .map(|entry| entry.launch.id)
            .collect::<BTreeSet<_>>();
        // A custom entry is listed; the unchecked built-in ones are not.
        assert!(listed.contains("mine"));
        assert!(listed.contains("registry.opencode"));
        assert!(!listed.contains("registry.gemini"));
        assert_eq!(listed.len(), 5);
        // Every lookup by id, which a restored pane or a model list uses, reads the whole catalogue.
        let all = registry::entries(&directory).unwrap();
        assert!(all.iter().any(|launch| launch.id == "registry.gemini"));
        assert_eq!(all.len(), registry::builtin_entries().unwrap().len() + 1);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
