use crate::Error;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use soyeht_protocol::{ConfigChoice, ConfigOption, ConfigType, ConfigValue};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Route {
    ConfigOption,
    SessionMode,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Options {
    pub values: Vec<ConfigOption>,
    pub routes: BTreeMap<String, Route>,
}

fn text(value: &Value, field: &str) -> Result<String, Error> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Error(format!("missing config field: {field}")))
}

fn identifier(value: &Value, field: &str) -> Result<String, Error> {
    let id = text(value, field)?;
    if id.is_empty() || id.chars().count() > 128 {
        return Err(Error(
            "native configuration identifier exceeds the public protocol limit".into(),
        ));
    }
    Ok(id)
}

fn optional_text(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_owned)
}

fn choice(value: &Value, group: Option<&str>) -> Result<ConfigChoice, Error> {
    let name = text(value, "name")?;
    Ok(ConfigChoice {
        value: identifier(value, "value")?,
        name: group.map_or_else(|| name.clone(), |group| format!("{group} · {name}")),
        description: optional_text(value, "description"),
    })
}

fn push_choice(
    choices: &mut Vec<ConfigChoice>,
    bytes: &mut usize,
    choice: ConfigChoice,
) -> Result<(), Error> {
    *bytes += serde_json::to_vec(&choice)?.len();
    if choices.len() == 256 || *bytes > soyeht_protocol::limits::JSON_BYTES / 2 {
        return Err(Error(
            "configuration choices exceed the metadata limit".into(),
        ));
    }
    choices.push(choice);
    Ok(())
}

fn option(value: &Value) -> Result<ConfigOption, Error> {
    let mut choices = Vec::new();
    let mut choice_bytes = 0;
    let kind = match value["type"].as_str() {
        Some("boolean") => ConfigType::Boolean,
        Some("select") => {
            let items = value["options"]
                .as_array()
                .ok_or_else(|| Error("missing select choices".into()))?;
            for item in items {
                if item.get("group").is_some() {
                    let group = text(item, "name")?;
                    for member in item["options"]
                        .as_array()
                        .ok_or_else(|| Error("missing grouped choices".into()))?
                    {
                        push_choice(
                            &mut choices,
                            &mut choice_bytes,
                            choice(member, Some(&group))?,
                        )?;
                    }
                } else {
                    push_choice(&mut choices, &mut choice_bytes, choice(item, None)?)?;
                }
            }
            let mut seen = BTreeSet::new();
            if choices.len() > 256 || choices.iter().any(|c| !seen.insert(c.value.clone())) {
                return Err(Error("too many or duplicate configuration choices".into()));
            }
            ConfigType::Select
        }
        _ => return Err(Error("unsupported configuration option type".into())),
    };
    let current = value
        .get("currentValue")
        .ok_or_else(|| Error("missing current config value".into()))?;
    if (kind == ConfigType::Boolean && !current.is_boolean())
        || (kind == ConfigType::Select && !current.is_string())
    {
        return Err(Error(
            "configuration current value has the wrong type".into(),
        ));
    }
    Ok(ConfigOption {
        id: identifier(value, "id")?,
        name: text(value, "name")?,
        description: optional_text(value, "description"),
        category: optional_text(value, "category"),
        r#type: kind,
        current_value: serde_json::from_value(current.clone())?,
        options: choices,
    })
}

impl Options {
    pub fn from_response(response: &Value) -> Result<Self, Error> {
        let mut result = Self::default();
        if let Some(options) = response.get("configOptions").filter(|v| !v.is_null()) {
            for value in options
                .as_array()
                .ok_or_else(|| Error("configOptions must be an array".into()))?
            {
                let option = option(value)?;
                if result.values.len() == 64
                    || result
                        .routes
                        .insert(option.id.clone(), Route::ConfigOption)
                        .is_some()
                {
                    return Err(Error("too many or duplicate configuration options".into()));
                }
                result.values.push(option);
                result.check_size()?;
            }
        }
        if let Some(modes) = response.get("modes").filter(|v| !v.is_null())
            && !result
                .values
                .iter()
                .any(|option| option.category.as_deref() == Some("mode") || option.id == "mode")
        {
            let available = modes["availableModes"]
                .as_array()
                .ok_or_else(|| Error("missing session modes".into()))?;
            let choices: Result<Vec<_>,Error> = available.iter().map(|mode| Ok(json!({"value":text(mode,"id")?,"name":text(mode,"name")?,"description":mode.get("description")}))).collect();
            result.values.push(option(&json!({"id":"mode","name":"Mode","category":"mode","type":"select","currentValue":text(modes,"currentModeId")?,"options":choices?}))?);
            result.routes.insert("mode".into(), Route::SessionMode);
        }
        result.check_size()?;
        Ok(result)
    }

    fn check_size(&self) -> Result<(), Error> {
        if self.values.len() > 64
            || serde_json::to_vec(&self.values)?.len() > soyeht_protocol::limits::JSON_BYTES / 2
        {
            return Err(Error("configuration metadata exceeds its limit".into()));
        }
        Ok(())
    }

    pub fn request(
        &self,
        session: &str,
        id: &str,
        value: &ConfigValue,
    ) -> Result<(&'static str, Value), Error> {
        let option = self
            .values
            .iter()
            .find(|option| option.id == id)
            .ok_or_else(|| Error("unknown configuration option".into()))?;
        let value = serde_json::to_value(value)?;
        match option.r#type {
            ConfigType::Boolean if value.is_boolean() => (),
            ConfigType::Select
                if option
                    .options
                    .iter()
                    .any(|choice| Some(choice.value.as_str()) == value.as_str()) => {}
            _ => {
                return Err(Error(
                    "configuration value is not an available choice".into(),
                ));
            }
        }
        if self.routes.get(id) == Some(&Route::SessionMode) {
            return Ok((
                "session/set_mode",
                json!({"sessionId":session,"modeId":value}),
            ));
        }
        let mut params = json!({"sessionId":session,"configId":id,"value":value});
        if option.r#type == ConfigType::Boolean {
            params["type"] = json!("boolean");
        }
        Ok(("session/set_config_option", params))
    }

    /// Apply only after a successful native response, never when sending the request.
    pub fn confirmed(
        &self,
        id: &str,
        value: &ConfigValue,
        response: &Value,
    ) -> Result<Self, Error> {
        if self.routes.get(id) == Some(&Route::SessionMode) {
            let mut next = self.clone();
            let option = next
                .values
                .iter_mut()
                .find(|option| option.id == id)
                .ok_or_else(|| Error("missing mode option".into()))?;
            option.current_value = value.clone();
            return Ok(next);
        }
        if !response["configOptions"].is_array() {
            return Err(Error(
                "configuration response omitted confirmed options".into(),
            ));
        }
        let mut next = Self::from_response(response)?;
        for option in &self.values {
            if self.routes.get(&option.id) == Some(&Route::SessionMode)
                && !next
                    .values
                    .iter()
                    .any(|item| item.id == option.id || item.category.as_deref() == Some("mode"))
            {
                next.values.push(option.clone());
                next.routes.insert(option.id.clone(), Route::SessionMode);
            }
        }
        next.check_size()?;
        if !next
            .values
            .iter()
            .any(|option| option.id == id && option.current_value == *value)
        {
            return Err(Error(
                "native response did not confirm the requested value".into(),
            ));
        }
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_choices_and_boolean_keep_native_ids_and_require_confirmation() {
        let response = json!({"configOptions":[
            {"id":"model","name":"Model","category":"model","type":"select","currentValue":"a","options":[{"group":"group","name":"Local","options":[{"value":"a","name":"A"},{"value":"b","name":"B"}]}]},
            {"id":"fast","name":"Fast","type":"boolean","currentValue":false}
        ]});
        let options = Options::from_response(&response).unwrap();
        assert_eq!(options.values[0].options[1].name, "Local · B");
        let value: ConfigValue = serde_json::from_value(json!("b")).unwrap();
        let (method, params) = options.request("session", "model", &value).unwrap();
        assert_eq!(method, "session/set_config_option");
        assert_eq!(params["value"], "b");
        assert!(options.confirmed("model", &value, &response).is_err());
        let mut confirmed = response.clone();
        confirmed["configOptions"][0]["currentValue"] = json!("b");
        assert_eq!(
            options
                .confirmed("model", &value, &confirmed)
                .unwrap()
                .values[0]
                .current_value,
            value
        );
        let boolean = serde_json::from_value(json!(true)).unwrap();
        assert_eq!(
            options.request("session", "fast", &boolean).unwrap().1["type"],
            "boolean"
        );
        assert!(options.request("session", "model", &boolean).is_err());
    }

    #[test]
    fn legacy_modes_use_the_native_mode_method_and_missing_categories_are_valid() {
        let options=Options::from_response(&json!({"configOptions":[{"id":"custom","name":"Custom","type":"boolean","currentValue":false}],"modes":{"currentModeId":"ask","availableModes":[{"id":"ask","name":"Ask"},{"id":"all","name":"Allow"}]}})).unwrap();
        let value = serde_json::from_value(json!("all")).unwrap();
        assert_eq!(
            options.request("session", "mode", &value).unwrap(),
            (
                "session/set_mode",
                json!({"sessionId":"session","modeId":"all"})
            )
        );
        assert_eq!(
            options
                .confirmed("mode", &value, &json!({}))
                .unwrap()
                .values[1]
                .current_value,
            value
        );
        assert!(Options::from_response(&json!({"configOptions":[{"id":"x","name":"X","type":"boolean","currentValue":"false"}]})).is_err());
        let next = options.confirmed("custom", &serde_json::from_value(json!(true)).unwrap(), &json!({"configOptions":[{"id":"custom","name":"Custom","type":"boolean","currentValue":true}]})).unwrap();
        assert_eq!(next.routes.get("mode"), Some(&Route::SessionMode));
        assert_eq!(
            next.request("session", "mode", &value).unwrap().0,
            "session/set_mode"
        );
    }

    #[test]
    fn public_configuration_ids_fail_explicitly_instead_of_breaking_client_decoding() {
        for id in [String::new(), "x".repeat(129)] {
            assert!(Options::from_response(&json!({"configOptions":[{"id":id,"name":"X","type":"boolean","currentValue":false}]})).is_err());
            assert!(Options::from_response(&json!({"configOptions":[{"id":"model","name":"Model","type":"select","currentValue":id,"options":[{"value":id,"name":"X"}]}]})).is_err());
        }
    }
}
