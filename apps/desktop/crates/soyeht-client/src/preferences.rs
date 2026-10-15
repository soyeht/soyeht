//! The settings Preferences shows: the harness new agents start with, and the permission mode
//! new panes of each harness start in. Both are engine settings, so every client shares them.

use crate::{Result, preference_requests, rpc::Rpc};
use serde_json::Value;
use soyeht_protocol::{SettingsGetResult, setting};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preferences {
    /// The harness the person chose as their default; `None` leaves the choice to readiness.
    pub preferred_harness: Option<String>,
    /// The permission mode new panes of a harness start in; a harness absent here keeps its own.
    pub default_modes: BTreeMap<String, String>,
}

impl Preferences {
    /// Folds in the setting `key` now holding `value`; false when it is not one of these.
    pub fn apply(&mut self, key: &str, value: &Value) -> bool {
        let text = value.as_str().filter(|text| !text.is_empty());
        if key == setting::HARNESS_PREFERRED_ID {
            self.preferred_harness = text.map(str::to_owned);
            return true;
        }
        let Some(harness) = setting::harness_default_mode_key_parameter(key) else {
            return false;
        };
        match text {
            Some(mode) => self
                .default_modes
                .insert(harness.to_owned(), mode.to_owned()),
            None => self.default_modes.remove(harness),
        };
        true
    }
}

/// Reads the default harness and the default permission mode of each of `harnesses`.
pub(crate) fn load(rpc: &Rpc, harnesses: &[String]) -> Result<Preferences> {
    let mut preferences = Preferences::default();
    let keys = std::iter::once(setting::HARNESS_PREFERRED_ID.to_owned()).chain(
        harnesses
            .iter()
            .map(|harness| setting::harness_default_mode_key(harness)),
    );
    for key in keys {
        let result: SettingsGetResult =
            serde_json::from_value(rpc.call(&preference_requests::setting_request(&key))?)?;
        preferences.apply(&result.key, &result.value);
    }
    Ok(preferences)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_fold_in_by_their_key_and_an_empty_value_clears_one() {
        let mut preferences = Preferences::default();
        assert!(preferences.apply("harness.preferredId", &json!("codex")));
        assert!(preferences.apply("harness.defaultMode.codex", &json!("accept_edits")));
        assert!(!preferences.apply("terminal.defaultCols", &json!(80)));
        assert_eq!(preferences.preferred_harness.as_deref(), Some("codex"));
        assert_eq!(preferences.default_modes["codex"], "accept_edits");
        assert!(preferences.apply("harness.defaultMode.codex", &json!("")));
        assert!(preferences.default_modes.is_empty());
        assert!(preferences.apply("harness.preferredId", &json!("")));
        assert_eq!(preferences.preferred_harness, None);
    }
}
