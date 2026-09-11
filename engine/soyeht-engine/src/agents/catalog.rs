use super::{registry, responses};
use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soyeht_protocol::{ErrorKind, HarnessInfo, ModelInfo, ModelListSource, limits};

fn page(items: Vec<Value>, mut fixed: Value, params: &Value) -> Result<Value> {
    let digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&json!([items, fixed])).expect("catalogue"))
    );
    let start = if let Some(cursor) = params["cursor"].as_str() {
        let (prior, index) = cursor
            .split_once('.')
            .ok_or_else(|| errors::error(ErrorKind::InvalidParams, "Invalid catalogue cursor"))?;
        if prior != digest {
            return Err(errors::error(
                ErrorKind::ResyncRequired,
                "Catalogue changed; reload the first page",
            ));
        }
        index
            .parse::<usize>()
            .ok()
            .filter(|index| *index < items.len())
            .ok_or_else(|| errors::error(ErrorKind::InvalidParams, "Invalid catalogue position"))?
    } else {
        0
    };
    let mut accepted = Vec::new();
    let mut bytes = serde_json::to_vec(&fixed)
        .expect("catalogue metadata")
        .len()
        + 512;
    let maximum = params["limit"].as_u64().unwrap_or(64) as usize;
    for item in items.iter().skip(start).take(maximum) {
        let size = serde_json::to_vec(item).expect("catalogue item").len() + 1;
        if bytes + size > limits::JSON_BYTES {
            if accepted.is_empty() {
                return Err(errors::error(
                    ErrorKind::InvalidParams,
                    "Catalogue entry exceeds frame capacity",
                ));
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

impl Core {
    pub fn agent_catalog(&self, method: &str, params: &Value) -> Result<Value> {
        let entries = registry::entries(&self.directory)?;
        if method == "harness.list" {
            let mut items = Vec::new();
            for entry in entries {
                let prepared = registry::prepare(&self.directory, &entry.id).ok();
                let known = prepared.as_ref().and_then(|(launch, digest)| {
                    self.agents
                        .values()
                        .filter(|a| a.state.launch == *launch && a.state.binary_digest == *digest)
                        .max_by_key(|a| a.state.last_update_ms)
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
                items.push(json!(HarnessInfo {
                    id: entry.id,
                    name: entry.name,
                    available: prepared.is_some(),
                    version: initialized.map(|i| i.version.clone()),
                    fingerprint: known.map(|a| a.state.fingerprint.clone()),
                    capabilities,
                    options: known
                        .map(|a| a.state.options.values.clone())
                        .unwrap_or_default(),
                    auth_methods: initialized
                        .map(|i| i.auth_methods.iter().map(|m| m.public.clone()).collect())
                        .unwrap_or_default()
                }));
            }
            return page(items, json!({}), params);
        }
        let harness = params["harness"].as_str().expect("validated harness");
        if !entries.iter().any(|entry| entry.id == harness) {
            return Err(errors::error(
                ErrorKind::HarnessUnavailable,
                "Harness is not configured",
            ));
        }
        let known = if let Some(pane) = params["paneId"].as_str() {
            let state = self
                .agents
                .get(pane)
                .filter(|a| {
                    a.state.launch.id == harness
                        && self.panes.get(pane).is_some_and(|p| !p.stored.closed)
                })
                .ok_or_else(|| {
                    errors::error(ErrorKind::NotFound, "Agent pane not found for this harness")
                })?;
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
        let items = options
            .iter()
            .find(|o| o.category.as_deref() == Some("model") || o.id == "model")
            .map(|option| {
                option
                    .options
                    .iter()
                    .map(|choice| {
                        json!(ModelInfo {
                            id: choice.value.clone(),
                            name: choice.name.clone(),
                            description: choice.description.clone()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        page(
            items,
            json!({"options":options,"source":if known.is_some() {ModelListSource::NativeSession} else {ModelListSource::NotProbed}}),
            params,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            page(changed, json!({}), &json!({"cursor":first["nextCursor"]}))
                .unwrap_err()
                .data
                .kind,
            ErrorKind::ResyncRequired
        );
    }
}
