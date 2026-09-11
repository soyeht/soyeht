use crate::{
    core::Core,
    errors::{self, Result},
};
use rusqlite::{OptionalExtension, params};
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
impl Core {
    pub fn setting(&self, key: &str) -> Result<Value> {
        let default = default_value(key)?;
        let raw: Option<String> = self
            .store
            .connection()
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|e| errors::storage(e.into()))?;
        raw.map(|r| {
            serde_json::from_str(&r)
                .map_err(|_| errors::error(ErrorKind::InternalError, "Invalid setting record"))
        })
        .unwrap_or(Ok(default))
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
        Ok(self.mutate(connection,"settings.set",params,|tx| {
            tx.execute("INSERT INTO settings(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value.to_string()])?;
            Ok((json!({"key":key,"value":value}),vec![("settings.changed".into(),json!({"key":key,"value":value}))]))
        })?.result)
    }
}
