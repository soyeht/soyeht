use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_protocol::{
    event, method,
    setting::{self, SettingError},
};

fn unknown_or_invalid(error: SettingError) -> errors::Error {
    errors::invalid(match error {
        SettingError::Unknown => "Unknown setting",
        SettingError::Invalid => "Invalid setting value",
    })
}

fn default_value(key: &str) -> Result<Value> {
    setting::default_value(key).map_err(unknown_or_invalid)
}

/// The permission mode a new pane of `harness` requests, if one was chosen.
pub(crate) fn default_agent_mode(store: &impl StorePort, harness: &str) -> Result<Option<String>> {
    Ok(read(store, &setting::harness_default_mode_key(harness))?
        .as_str()
        .filter(|mode| !mode.is_empty())
        .map(str::to_owned))
}

pub(crate) fn read(store: &impl StorePort, key: &str) -> Result<Value> {
    let default = default_value(key)?;
    store
        .setting(key)
        .map_err(errors::storage)
        .map(|value| value.unwrap_or(default))
}

impl Core {
    pub fn setting(&self, key: &str) -> Result<Value> {
        read(&self.store, key)
    }
    pub fn setting_set(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let key = errors::param_str(params, "key")?;
        let value = &params["value"];
        setting::validate(key, value).map_err(unknown_or_invalid)?;
        Ok(self
            .mutate(connection, method::SETTINGS_SET, params, |tx| {
                tx.set_setting(key, value)?;
                Ok((
                    json!({"key":key,"value":value}),
                    vec![(
                        event::SETTINGS_CHANGED.into(),
                        json!({"key":key,"value":value}),
                    )],
                ))
            })?
            .result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::testing::MemoryStore;

    /// Every setting of the contract, with a family key filled in.
    fn keys() -> Vec<String> {
        let mut keys: Vec<String> = setting::KEYS.iter().map(|key| (*key).to_owned()).collect();
        keys.push(setting::harness_default_mode_key("test-agent"));
        keys
    }

    #[test]
    fn every_setting_of_the_contract_reads_back_its_default_then_what_was_stored() {
        let mut store = MemoryStore::default();
        for key in keys() {
            let default = setting::default_value(&key).unwrap();
            assert_eq!(read(&store, &key).unwrap(), default, "{key}");
            store.settings.insert(key.clone(), json!("stored"));
            assert_eq!(read(&store, &key).unwrap(), json!("stored"), "{key}");
        }
    }

    #[test]
    fn a_key_the_contract_does_not_list_is_an_unknown_setting() {
        let store = MemoryStore::default();
        for key in [
            "web.openLocalLinksInSoyeht",
            "harness.defaultMode.",
            "terminal",
        ] {
            let error = read(&store, key).unwrap_err();
            assert!(
                matches!(&error, errors::Error::InvalidParams(message) if message == "Unknown setting"),
                "{key}: {error:?}"
            );
        }
    }

    #[test]
    fn a_value_out_of_bounds_is_an_invalid_setting_value() {
        assert!(matches!(
            setting::validate(setting::TERMINAL_DEFAULT_COLS, &json!(0)).map_err(unknown_or_invalid),
            Err(errors::Error::InvalidParams(message)) if message == "Invalid setting value"
        ));
    }
}
