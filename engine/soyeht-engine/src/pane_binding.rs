//! The binding of a terminal pane (ADR 0165).
//!
//! Every process the pane runs may act for the pane, because its environment carries the binding
//! (`PANE_BINDING_ENV`) and a connection that presents it in `hello` is granted the messaging
//! methods for that pane alone. The binding is derived, not stored: a keyed hash, made with a key
//! of this engine, of the pane's id, the key of the pane's process instance and its generation.
//!
//! - It ends with the process. A restart, a recovery or a harness switch gives the pane another
//!   instance key and generation, so the binding of the old process no longer verifies, and the
//!   connections that presented it are refused at their next request.
//! - A start asked again for the same instance key gets the same binding, which the supervisor
//!   needs: it hands back the process the first start made, with the environment that start sent.
//! - The engine's database holds no binding. The tag is recomputed from the key of the engine
//!   and the pane's instance key and generation, which the store holds, so a restart of the
//!   engine verifies the bindings of the panes that outlived it. The binding itself is kept
//!   where the process is: the supervisor records the environment of every process it holds in
//!   its registry file (`ptyd-registry.json`, readable by its owner only), so the bindings of
//!   the running terminal panes are in that file, and in the environment of each process.
//!
//! The binding names its pane in the clear (`PANE.TAG`), so a check is one keyed hash and not one
//! per pane.

use crate::{
    agents::roster::Agents,
    errors::{self, Result},
    model::{RunningPane, StoredPane, TerminalInstance},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::SigningKey;
use ring::hmac;
use sha2::{Digest, Sha256};
use soyeht_protocol::{PaneMode, Role};
use std::collections::BTreeMap;
use zeroize::Zeroize;

/// Separates this use of the engine's key from every other.
const LABEL: &[u8] = b"soyeht terminal pane binding v1";

/// What the keyed hash covers: a pane's identity as the supervisor and the store know it.
pub(crate) struct Instance<'a> {
    pub(crate) pane_id: &'a str,
    pub(crate) instance_key: &'a str,
    pub(crate) generation: u32,
}

impl Instance<'_> {
    fn message(&self) -> Vec<u8> {
        // NUL cannot appear in a pane id or an instance key, so the fields cannot run together.
        format!(
            "{}\0{}\0{}",
            self.pane_id, self.instance_key, self.generation
        )
        .into_bytes()
    }
}

fn key(engine: &SigningKey) -> hmac::Key {
    let mut seed = engine.to_bytes();
    let mut derived = Sha256::new();
    derived.update(LABEL);
    derived.update([0]);
    derived.update(seed);
    seed.zeroize();
    hmac::Key::new(hmac::HMAC_SHA256, &derived.finalize())
}

/// The binding of the process `instance` of a terminal pane.
pub(crate) fn issue(engine: &SigningKey, instance: &Instance) -> String {
    let tag = hmac::sign(&key(engine), &instance.message());
    format!("{}.{}", instance.pane_id, URL_SAFE_NO_PAD.encode(tag))
}

/// The pane a binding says it is for. It is a claim, which [`verify`] checks.
pub(crate) fn claimed_pane(candidate: &str) -> Option<&str> {
    candidate.split_once('.').map(|(pane, _)| pane)
}

/// Whether `candidate` is the binding of `instance`.
pub(crate) fn verify(engine: &SigningKey, candidate: &str, instance: &Instance) -> bool {
    let Some((pane, tag)) = candidate.split_once('.') else {
        return false;
    };
    let Ok(tag) = URL_SAFE_NO_PAD.decode(tag) else {
        return false;
    };
    pane == instance.pane_id && hmac::verify(&key(engine), &instance.message(), &tag).is_ok()
}

fn instance(pane: &StoredPane) -> Instance<'_> {
    Instance {
        pane_id: &pane.pane.pane_id,
        instance_key: &pane.instance_key,
        generation: pane.pane.generation,
    }
}

/// The binding in the environment of the process the terminal pane `pane` runs.
pub(crate) fn binding_of(engine: &SigningKey, pane: &StoredPane) -> String {
    issue(engine, &instance(pane))
}

/// The pane and process a presented binding speaks for.
pub(crate) struct Bound {
    pub(crate) role: Role,
    pub(crate) pane: String,
    /// For a terminal pane, the process the binding belongs to.
    pub(crate) terminal: Option<TerminalInstance>,
}

/// Who `candidate` is the binding of: an agent pane (its checkpoint holds the binding), or the
/// open terminal pane whose current process it was made for.
pub(crate) fn identify(
    engine: &SigningKey,
    agents: &Agents,
    panes: &BTreeMap<String, RunningPane>,
    candidate: &str,
) -> Result<Bound> {
    let mut found = None;
    for (pane, running) in agents.iter() {
        let active = panes.get(pane).is_some_and(|saved| !saved.stored.closed);
        if active && running.state.binding.matches(candidate) {
            if found.is_some() {
                return Err(errors::internal("Agent binding is not unique"));
            }
            let role = match running.state.mcp_role {
                Role::Agent => Role::Agent,
                Role::Chief => Role::Chief,
                _ => return Err(errors::internal("Agent checkpoint has an invalid role")),
            };
            found = Some(Bound {
                role,
                pane: pane.clone(),
                terminal: None,
            });
        }
    }
    if found.is_none()
        && let Some(running) = claimed_pane(candidate).and_then(|pane| panes.get(pane))
        && let pane = &running.stored
        && !pane.closed
        && pane.pane.mode == PaneMode::Terminal
        && verify(engine, candidate, &instance(pane))
    {
        found = Some(Bound {
            role: Role::Agent,
            pane: pane.pane.pane_id.clone(),
            terminal: Some(TerminalInstance {
                pane_id: pane.pane.pane_id.clone(),
                instance_key: pane.instance_key.clone(),
                generation: pane.pane.generation,
            }),
        });
    }
    found.ok_or_else(|| errors::unauthorized("Unknown agent binding"))
}

/// A connection that presented a terminal pane's binding speaks only while the pane is open and
/// runs the process the binding was made for.
pub(crate) fn require_current(
    panes: &BTreeMap<String, RunningPane>,
    bound: &TerminalInstance,
) -> Result<()> {
    let current = panes.get(&bound.pane_id).is_some_and(|running| {
        let pane = &running.stored;
        !pane.closed
            && pane.instance_key == bound.instance_key
            && pane.pane.generation == bound.generation
    });
    if current {
        Ok(())
    } else {
        Err(errors::unauthorized("Stale agent binding"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(byte: u8) -> SigningKey {
        SigningKey::from_bytes(&[byte; 32])
    }

    const INSTANCE: Instance = Instance {
        pane_id: "5d0f3a52-1111-4222-8333-444444444444",
        instance_key: "key-1",
        generation: 3,
    };

    #[test]
    fn a_binding_is_the_same_each_time_and_names_its_pane() {
        let binding = issue(&engine(1), &INSTANCE);
        assert_eq!(binding, issue(&engine(1), &INSTANCE));
        assert!(binding.starts_with("5d0f3a52-1111-4222-8333-444444444444."));
        assert_eq!(claimed_pane(&binding), Some(INSTANCE.pane_id));
        assert!(verify(&engine(1), &binding, &INSTANCE));
        assert_eq!(claimed_pane("no-dot"), None);
    }

    #[test]
    fn a_binding_ends_with_its_process_and_belongs_to_one_pane_and_one_engine() {
        let binding = issue(&engine(1), &INSTANCE);
        let other = |pane_id, instance_key, generation| {
            verify(
                &engine(1),
                &binding,
                &Instance {
                    pane_id,
                    instance_key,
                    generation,
                },
            )
        };
        assert!(other(INSTANCE.pane_id, "key-1", 3));
        // A new process of the pane: another instance key, or the same key at another generation.
        assert!(!other(INSTANCE.pane_id, "key-2", 3));
        assert!(!other(INSTANCE.pane_id, "key-1", 4));
        // Another pane cannot present it, even with the same key and generation.
        assert!(!other("another-pane", "key-1", 3));
        // Another engine's key makes another binding.
        assert!(!verify(&engine(2), &binding, &INSTANCE));
    }

    #[test]
    fn malformed_candidates_never_verify() {
        let binding = issue(&engine(1), &INSTANCE);
        let (pane, tag) = binding.split_once('.').unwrap();
        for candidate in [
            String::new(),
            ".".into(),
            pane.into(),
            format!("{pane}."),
            format!("{pane}.{tag}x"),
            format!("{pane}.!!!"),
            format!("other.{tag}"),
            format!("{pane}.{}", tag.to_uppercase()),
        ] {
            assert!(!verify(&engine(1), &candidate, &INSTANCE), "{candidate:?}");
        }
    }
}
