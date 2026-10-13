use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_protocol::{event, method};

pub const PREFERRED_HARNESS_KEY: &str = "harness.preferredId";
/// Followed by a harness ID: the permission mode new panes of that harness
/// start in, as the harness names it. Empty keeps the harness's own default.
const DEFAULT_MODE_PREFIX: &str = "harness.defaultMode.";

fn default_value(key: &str) -> Result<Value> {
    match key {
        "record.default" => Ok(json!(true)),
        "terminal.defaultCols" => Ok(json!(80)),
        "terminal.defaultRows" => Ok(json!(24)),
        PREFERRED_HARNESS_KEY => Ok(json!("")),
        key if key
            .strip_prefix(DEFAULT_MODE_PREFIX)
            .is_some_and(|harness| !harness.is_empty() && harness.len() <= 128) =>
        {
            Ok(json!(""))
        }
        _ => Err(errors::invalid("Unknown setting")),
    }
}

/// The permission mode a new pane of `harness` requests, if one was chosen.
pub(crate) fn default_agent_mode(store: &impl StorePort, harness: &str) -> Result<Option<String>> {
    Ok(read(store, &format!("{DEFAULT_MODE_PREFIX}{harness}"))?
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
        let default = default_value(key)?;
        if (default.is_boolean() && !value.is_boolean())
            || (default.is_number() && !value.as_u64().is_some_and(|n| (1..=1000).contains(&n)))
            || (default.is_string()
                && !value
                    .as_str()
                    .is_some_and(|id| id.len() <= 128 && !id.contains('\0')))
        {
            return Err(errors::invalid("Invalid setting value"));
        }
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
