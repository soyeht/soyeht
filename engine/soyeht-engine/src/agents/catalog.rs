use super::{registry, responses};
use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soyeht_protocol::{
    HarnessEnvironmentIssue, HarnessInfo, HarnessSetupState, ModelInfo, ModelListSource,
    ModelRefusal, ProbeResult, limits,
};
use soyeht_store::agents::AgentModelRefusal;

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
    capabilities: &[soyeht_protocol::ProbeCapability],
    initialized: bool,
    resolved: bool,
    issue: bool,
) -> HarnessSetupState {
    if capabilities
        .iter()
        .any(|capability| capability.result == ProbeResult::Unauthenticated)
    {
        HarnessSetupState::SignIn
    } else if capabilities
        .iter()
        .any(|capability| capability.result == ProbeResult::Inconclusive)
        || (resolved && (!initialized || capabilities.is_empty()))
        || issue
    {
        HarnessSetupState::CheckSetup
    } else if resolved {
        HarnessSetupState::Ready
    } else {
        HarnessSetupState::NotInstalled
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

impl Core {
    pub fn agent_catalog(&self, method: &str, params: &Value) -> Result<Value> {
        if method == "harness.list" {
            if params["rescan"].as_bool() == Some(true) && params["cursor"].is_null() {
                super::environment::invalidate_launch_path();
            }
            let issue = environment_issue();
            let entries = registry::catalogue_entries(&self.directory)?;
            let mut items = Vec::new();
            for entry in entries {
                let resolved = registry::resolve_entry(entry.clone());
                let has_session = self
                    .agents
                    .values()
                    .any(|agent| agent.state.launch.id == entry.launch.id);
                let prepared = has_session
                    .then(|| registry::prepare_entry(entry.clone()).ok())
                    .flatten();
                let known = prepared.as_ref().and_then(|(launch, digest)| {
                    self.agents
                        .values()
                        .filter(|agent| {
                            agent.state.launch == *launch && agent.state.binary_digest == *digest
                        })
                        .max_by_key(|agent| agent.state.last_update_ms)
                });
                let mut capabilities = Vec::new();
                if let Some(known) = known {
                    for method in responses::PROBED_METHODS {
                        if let Some(capability) = self
                            .store
                            .agent_probe(&known.state.fingerprint, method)
                            .map_err(errors::storage)?
                        {
                            capabilities.push(capability);
                        }
                    }
                }
                let initialized = known.and_then(|a| a.state.initialized.as_ref());
                let installed = registry::locally_installed(&entry, known.is_some());
                let setup_state = setup_state(
                    &capabilities,
                    initialized.is_some(),
                    resolved.is_some(),
                    issue.is_some(),
                );
                items.push(json!(HarnessInfo {
                    id: entry.launch.id,
                    name: entry.launch.name,
                    available: resolved.is_some(),
                    installed: Some(installed),
                    version: initialized.map(|i| i.version.clone()).or_else(|| (!entry
                        .launch
                        .version
                        .is_empty())
                    .then(|| entry.launch.version.clone())),
                    fingerprint: known.map(|a| a.state.fingerprint.clone()),
                    capabilities,
                    options: known
                        .map(|a| a.state.options.values.clone())
                        .unwrap_or_default(),
                    auth_methods: initialized
                        .map(|i| i.auth_methods.iter().map(|m| m.public.clone()).collect())
                        .unwrap_or_default(),
                    source: Some(entry.source),
                    setup_state: Some(setup_state)
                }));
            }
            return page(items, json!({"environmentIssue":issue}), params);
        }
        let entries = registry::entries(&self.directory)?;
        let harness = errors::required(params["harness"].as_str(), "Harness ID is required")?;
        if !entries.iter().any(|entry| entry.id == harness) {
            return Err(errors::harness_unavailable("Harness is not configured"));
        }
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
            let (launch, digest) = registry::prepare(&self.directory, harness)?;
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
            .agent_model_refusals(harness)
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
    use soyeht_protocol::ProbeCapability;

    fn capability(result: ProbeResult) -> ProbeCapability {
        ProbeCapability {
            method: "session/prompt".into(),
            result,
            checked_at: "2026-09-15T00:00:00Z".into(),
        }
    }

    #[test]
    fn setup_state_requires_real_probe_facts_before_ready() {
        assert_eq!(
            setup_state(&[], false, true, false),
            HarnessSetupState::CheckSetup
        );
        assert_eq!(
            setup_state(&[capability(ProbeResult::Worked)], true, true, false),
            HarnessSetupState::Ready
        );
        assert_eq!(
            setup_state(&[capability(ProbeResult::Inconclusive)], true, true, false),
            HarnessSetupState::CheckSetup
        );
        assert_eq!(
            setup_state(
                &[capability(ProbeResult::Unauthenticated)],
                true,
                true,
                false
            ),
            HarnessSetupState::SignIn
        );
        assert_eq!(
            setup_state(&[], false, false, false),
            HarnessSetupState::NotInstalled
        );
        assert_eq!(
            setup_state(&[], false, false, true),
            HarnessSetupState::CheckSetup
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
}
