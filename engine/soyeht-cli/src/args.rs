//! What the command files share: the invalid-arguments error, the typed engine call,
//! the `--name value` flag parser and the small parameter builders.

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};
use soyeht_cli::{Client, Error, Result};
use std::collections::BTreeMap;

pub fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidArguments(message.into())
}

pub fn rpc<T: DeserializeOwned + Serialize>(
    client: &mut Client,
    method: &str,
    params: Value,
) -> Result<Value> {
    Ok(serde_json::to_value(client.request::<T>(method, params)?)?)
}

pub fn number(value: &str, name: &str) -> Result<u32> {
    value
        .parse()
        .map_err(|_| invalid(format!("{name} must be an integer")))
}

pub fn boolean(value: &str, name: &str) -> Result<bool> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid(format!("{name} must be true or false"))),
    }
}

pub fn insert(params: &mut Map<String, Value>, name: &str, value: &str) {
    params.insert(name.into(), json!(value));
}

pub fn optional(params: &mut Map<String, Value>, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        insert(params, name, value);
    }
}

/// The `--name value` pairs of a command, each allowed once.
pub struct Flags {
    values: BTreeMap<String, String>,
}

impl Flags {
    pub fn parse(args: &[&str], allowed: &[&str]) -> Result<Self> {
        let mut values = BTreeMap::new();
        let mut index = 0;
        while index < args.len() {
            let name = args[index];
            if !allowed.contains(&name) {
                return Err(invalid(format!("Unknown option: {name}")));
            }
            let value = args
                .get(index + 1)
                .ok_or_else(|| invalid(format!("Missing value for {name}")))?;
            if values
                .insert(name.to_owned(), (*value).to_owned())
                .is_some()
            {
                return Err(invalid(format!("Repeated option: {name}")));
            }
            index += 2;
        }
        Ok(Self { values })
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub fn required(&self, name: &str) -> Result<&str> {
        self.get(name)
            .ok_or_else(|| invalid(format!("Missing required option: {name}")))
    }

    pub fn number(&self, name: &str) -> Result<Option<u32>> {
        self.get(name).map(|value| number(value, name)).transpose()
    }

    pub fn boolean(&self, name: &str) -> Result<Option<bool>> {
        self.get(name).map(|value| boolean(value, name)).transpose()
    }
}
