//! The protocol sign-in method remembered for a harness (ADR 0100).
//!
//! Some harnesses ask every agent process to answer `authenticate`. The method the person
//! pressed is kept per harness, so each new process of any pane of that harness, and each
//! switch candidate, is authenticated with it before it opens a session. Only the method id is
//! kept: never a credential.

use crate::{
    errors::{self, Result},
    ports::StorePort,
};
use soyeht_acp::session::{AuthKind, Initialization};
use soyeht_store::{database::Change, harness_sign_in};

/// A change of the remembered choice, committed with the agent change that caused it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SignInChange {
    /// The person pressed this protocol sign-in method.
    Remember { harness: String, method: String },
    /// The agent refused the method, or the person signed out.
    Forget { harness: String },
}

impl SignInChange {
    pub(crate) fn remember(harness: &str, method: &str) -> Self {
        Self::Remember {
            harness: harness.into(),
            method: method.into(),
        }
    }

    pub(crate) fn forget(harness: &str) -> Self {
        Self::Forget {
            harness: harness.into(),
        }
    }

    pub(super) fn persist(&self, tx: &Change<'_>) -> soyeht_store::database::Result<()> {
        match self {
            Self::Remember { harness, method } => harness_sign_in::remember(tx, harness, method),
            Self::Forget { harness } => harness_sign_in::forget(tx, harness),
        }
    }
}

/// The method a new agent process of `harness` is authenticated with: the one the person chose,
/// while this agent still offers it as a protocol method.
pub(crate) fn method_to_send(
    store: &impl StorePort,
    harness: &str,
    initialized: &Initialization,
) -> Result<Option<String>> {
    let chosen = store.harness_sign_in(harness).map_err(errors::storage)?;
    Ok(chosen.filter(|chosen| offers_protocol_method(initialized, chosen)))
}

fn offers_protocol_method(initialized: &Initialization, method: &str) -> bool {
    initialized
        .auth_methods
        .iter()
        .any(|offered| offered.kind == AuthKind::Agent && offered.public.id == method)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::testing::MemoryStore;
    use serde_json::json;

    fn offering(methods: serde_json::Value) -> Initialization {
        Initialization::parse(&json!({"protocolVersion":1,"authMethods":methods})).unwrap()
    }

    fn store_with(harness: &str, method: &str) -> MemoryStore {
        let mut store = MemoryStore::default();
        store.sign_ins.insert(harness.into(), method.into());
        store
    }

    #[test]
    fn nothing_is_sent_for_a_harness_whose_person_chose_no_method() {
        let initialized = offering(json!([{"id":"login","name":"Login"}]));
        let store = store_with("other", "login");
        assert_eq!(
            method_to_send(&store, "per-process-auth", &initialized).unwrap(),
            None
        );
    }

    #[test]
    fn the_harness_choice_is_sent_while_the_agent_offers_it_as_a_protocol_method() {
        let initialized = offering(json!([{"id":"login","name":"Login"}]));
        let store = store_with("per-process-auth", "login");
        assert_eq!(
            method_to_send(&store, "per-process-auth", &initialized).unwrap(),
            Some("login".into())
        );
    }

    #[test]
    fn a_method_the_agent_no_longer_offers_or_offers_only_as_a_terminal_is_not_sent() {
        let store = store_with("per-process-auth", "login");
        for methods in [
            json!([{"id":"other","name":"Other"}]),
            json!([]),
            json!([{"id":"login","name":"Login","type":"terminal"}]),
        ] {
            assert_eq!(
                method_to_send(&store, "per-process-auth", &offering(methods)).unwrap(),
                None
            );
        }
    }
}
