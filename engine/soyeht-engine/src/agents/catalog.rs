use super::{registry, responses, state::Phase};
use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soyeht_protocol::{
    HarnessEnvironmentIssue, HarnessInfo, HarnessSetupReason, HarnessSetupState, HarnessSource,
    ModelInfo, ModelListSource, ModelRefusal, limits,
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

fn environment_issue() -> Option<HarnessEnvironmentIssue> {
    super::environment::launch_path_resolution()
        .issue
        .map(wire_environment_issue)
}

fn setup_state(
    resolved: bool,
    installed: bool,
    package_runner: bool,
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
    if let Some(recorded) = recorded {
        return (recorded.state.clone(), recorded.reason.clone());
    }
    if !installed {
        return if package_runner {
            (
                HarnessSetupState::NotChecked,
                Some(HarnessSetupReason::RegistryPackageUnverified),
            )
        } else {
            (
                HarnessSetupState::NotInstalled,
                Some(HarnessSetupReason::ExecutableMissing),
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

pub(crate) fn prepare_catalog(
    directory: PathBuf,
    method: &str,
    params: &Value,
    session_harnesses: BTreeSet<String>,
) -> Result<PreparedCatalog> {
    if method == "harness.list" {
        let started = Instant::now();
        if params["rescan"].as_bool() == Some(true) && params["cursor"].is_null() {
            super::environment::invalidate_launch_path();
        }
        let issue = environment_issue();
        let environment_ms = started.elapsed().as_millis();
        let mut resolve_ms = 0;
        let mut prepare_ms = 0;
        let mut installed_ms = 0;
        let entries = registry::catalogue_entries(&directory)?
            .into_iter()
            .map(|entry| {
                let step = Instant::now();
                let resolved = registry::resolve_entry(entry.clone());
                resolve_ms += step.elapsed().as_millis();
                let step = Instant::now();
                let prepared = session_harnesses
                    .contains(&entry.launch.id)
                    .then(|| registry::prepare_entry(entry.clone()).ok())
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
        eprintln!(
            "{}",
            json!({
                "event":"engine.catalog_prepared",
                "method":"harness.list",
                "entries":entries.len(),
                "environmentMs":environment_ms,
                "resolveMs":resolve_ms,
                "prepareMs":prepare_ms,
                "installedMs":installed_ms,
                "totalMs":started.elapsed().as_millis()
            })
        );
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
    } else {
        Some(registry::prepare(&directory, harness)?)
    };
    eprintln!(
        "{}",
        json!({
            "event":"engine.catalog_prepared",
            "method":"model.list",
            "totalMs":started.elapsed().as_millis()
        })
    );
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
        && matches!(entry.launch.command.as_str(), "npx" | "uvx");
    let (setup_state, setup_reason) = setup_state(
        resolved.is_some(),
        installed,
        package_runner,
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
        last_ready_at_ms: recorded.and_then(|value| value.last_ready_ms.map(|ms| ms.to_string()))
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
            return page(items, json!({"environmentIssue":issue}), params);
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
            let (launch, digest) = prepared
                .ok_or_else(|| errors::internal("Model catalogue launch was not prepared"))?;
            self.agents
                .values()
                .filter(|a| a.state.launch == launch && a.state.binary_digest == digest)
                .max_by_key(|a| a.state.last_update_ms)
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
            setup_state(true, true, false, false, None, None),
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
            setup_state(true, true, false, false, Some(&ready), None),
            (HarnessSetupState::Ready, None)
        );
        assert_eq!(
            setup_state(true, true, false, false, None, Some(&Phase::NeedsLogin)),
            (
                HarnessSetupState::SignIn,
                Some(HarnessSetupReason::AuthenticationRequired)
            )
        );
        assert_eq!(
            setup_state(false, true, false, false, Some(&ready), None),
            (
                HarnessSetupState::NotInstalled,
                Some(HarnessSetupReason::ExecutableMissing)
            )
        );
        assert_eq!(
            setup_state(false, true, false, true, Some(&ready), None),
            (
                HarnessSetupState::NotInstalled,
                Some(HarnessSetupReason::EnvironmentUnavailable)
            )
        );
        assert_eq!(
            setup_state(true, false, true, false, None, None),
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
            setup_state(true, false, true, false, Some(&login), None),
            (
                HarnessSetupState::SignIn,
                Some(HarnessSetupReason::AuthenticationRequired)
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
}
