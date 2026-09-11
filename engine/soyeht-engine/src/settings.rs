use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_protocol::ErrorKind;

fn default_value(key: &str) -> Result<Value> {
    match key {
        "record.default" => Ok(json!(true)),
        "terminal.defaultCols" => Ok(json!(80)),
        "terminal.defaultRows" => Ok(json!(24)),
        _ => Err(errors::error(ErrorKind::InvalidParams, "Unknown setting")),
    }
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
        let key = params["key"].as_str().expect("key");
        let value = &params["value"];
        let default = default_value(key)?;
        if (default.is_boolean() && !value.is_boolean())
            || (default.is_number() && !value.as_u64().is_some_and(|n| (1..=1000).contains(&n)))
        {
            return Err(errors::error(
                ErrorKind::InvalidParams,
                "Invalid setting value",
            ));
        }
        Ok(self
            .mutate(connection, "settings.set", params, |tx| {
                tx.set_setting(key, value)?;
                Ok((
                    json!({"key":key,"value":value}),
                    vec![("settings.changed".into(), json!({"key":key,"value":value}))],
                ))
            })?
            .result)
    }
}
