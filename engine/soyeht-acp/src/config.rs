use crate::Error;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use soyeht_protocol::{ConfigChoice, ConfigOption, ConfigType, ConfigValue};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Route {
    ConfigOption,
    SessionMode,
    /// Enforced by the ACP client itself; the agent never sees the value.
    Client,
}

pub use soyeht_protocol::client_policy::PERMISSION_MODE_CATEGORY;
/// Mode kind for presets that approve everything without asking.
pub const FULL_ACCESS_KIND: &str = "full_access";
/// Adapters that predate `_meta.kind` still name their full-access presets.
const FULL_ACCESS_MODE_IDS: [&str; 3] = ["bypassPermissions", "agent-full-access", "full-access"];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Options {
    pub values: Vec<ConfigOption>,
    pub routes: BTreeMap<String, Route>,
}

/// Whether an adapter that predates `_meta.kind` names this mode full access.
pub fn is_known_full_access_id(mode: &str) -> bool {
    FULL_ACCESS_MODE_IDS.contains(&mode)
}

fn text(value: &Value, field: &str) -> Result<String, Error> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Error::MissingConfigField(field.to_owned()))
}

fn identifier(value: &Value, field: &str) -> Result<String, Error> {
    let id = text(value, field)?;
    if id.is_empty() || id.chars().count() > 128 {
        return Err(Error::ConfigurationIdentifierTooLong);
    }
    Ok(id)
}

fn optional_text(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_owned)
}

fn choice(value: &Value, group: Option<&str>) -> Result<ConfigChoice, Error> {
    let name = text(value, "name")?;
    let kind = value["_meta"]["kind"]
        .as_str()
        .filter(|kind| !kind.is_empty() && kind.chars().count() <= 64)
        .map(str::to_owned);
    Ok(ConfigChoice {
        value: identifier(value, "value")?,
        name: group.map_or_else(|| name.clone(), |group| format!("{group} · {name}")),
        description: optional_text(value, "description"),
        kind,
    })
}

fn classify_full_access(option: &mut ConfigOption) {
    if !option.is_permission_mode() {
        return;
    }
    for choice in &mut option.options {
        if choice.kind.is_none() && is_known_full_access_id(&choice.value) {
            choice.kind = Some(FULL_ACCESS_KIND.into());
        }
    }
}

fn push_choice(
    choices: &mut Vec<ConfigChoice>,
    bytes: &mut usize,
    choice: ConfigChoice,
) -> Result<(), Error> {
    *bytes += serde_json::to_vec(&choice)?.len();
    if choices.len() == 256 || *bytes > soyeht_protocol::limits::JSON_BYTES / 2 {
        return Err(Error::ConfigurationChoicesTooLarge);
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
                .ok_or(Error::MissingSelectChoices)?;
            for item in items {
                if item.get("group").is_some() {
                    let group = text(item, "name")?;
                    for member in item["options"]
                        .as_array()
                        .ok_or(Error::MissingGroupedChoices)?
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
                return Err(Error::DuplicateConfigurationChoices);
            }
            ConfigType::Select
        }
        _ => return Err(Error::UnsupportedConfigurationType),
    };
    let current = value
        .get("currentValue")
        .ok_or(Error::MissingCurrentConfigValue)?;
    if (kind == ConfigType::Boolean && !current.is_boolean())
        || (kind == ConfigType::Select && !current.is_string())
    {
        return Err(Error::ConfigurationValueType);
    }
    let mut option = ConfigOption {
        id: identifier(value, "id")?,
        name: text(value, "name")?,
        description: optional_text(value, "description"),
        category: optional_text(value, "category"),
        r#type: kind,
        current_value: serde_json::from_value(current.clone())?,
        options: choices,
    };
    classify_full_access(&mut option);
    Ok(option)
}

impl Options {
    pub fn from_response(response: &Value) -> Result<Self, Error> {
        let mut result = Self::default();
        if let Some(options) = response.get("configOptions").filter(|v| !v.is_null()) {
            for value in options
                .as_array()
                .ok_or(Error::ConfigurationOptionsNotArray)?
            {
                let option = option(value)?;
                if result.values.len() == 64
                    || result
                        .routes
                        .insert(option.id.clone(), Route::ConfigOption)
                        .is_some()
                {
                    return Err(Error::DuplicateConfigurationOptions);
                }
                result.values.push(option);
                result.check_size()?;
            }
        }
        if let Some(modes) = response.get("modes").filter(|v| !v.is_null())
            && result.mode().is_none()
        {
            let available = modes["availableModes"]
                .as_array()
                .ok_or(Error::MissingSessionModes)?;
            let choices: Result<Vec<_>,Error> = available.iter().map(|mode| Ok(json!({"value":text(mode,"id")?,"name":text(mode,"name")?,"description":mode.get("description"),"_meta":mode.get("_meta")}))).collect();
            result.values.push(option(&json!({"id":PERMISSION_MODE_CATEGORY,"name":"Mode","category":PERMISSION_MODE_CATEGORY,"type":"select","currentValue":text(modes,"currentModeId")?,"options":choices?}))?);
            result
                .routes
                .insert(PERMISSION_MODE_CATEGORY.into(), Route::SessionMode);
        }
        result.check_size()?;
        Ok(result)
    }

    /// The permission-mode selector, whichever mechanism announced it.
    pub fn mode(&self) -> Option<&ConfigOption> {
        self.values
            .iter()
            .find(|option| option.is_permission_mode())
    }

    pub fn mode_mut(&mut self) -> Option<&mut ConfigOption> {
        self.values
            .iter_mut()
            .find(|option| option.is_permission_mode())
    }

    /// The selected permission mode's native identifier.
    pub fn current_mode(&self) -> Option<&str> {
        self.mode().and_then(|option| match &option.current_value {
            ConfigValue::V0(value) => Some(value.as_str()),
            ConfigValue::V1(_) => None,
        })
    }

    /// Whether `value` is one of the announced permission modes.
    pub fn offers_mode(&self, value: &str) -> bool {
        self.mode()
            .is_some_and(|option| option.options.iter().any(|choice| choice.value == value))
    }

    pub fn route(&self, id: &str) -> Option<&Route> {
        self.routes.get(id)
    }

    /// Adds a mode selector the client enforces when the agent announces none.
    pub fn ensure_mode(&mut self, fallback: impl FnOnce() -> ConfigOption) -> Result<(), Error> {
        if self.mode().is_some() {
            return Ok(());
        }
        let option = fallback();
        if self
            .routes
            .insert(option.id.clone(), Route::Client)
            .is_some()
        {
            return Err(Error::DuplicateConfigurationOptions);
        }
        self.values.push(option);
        self.check_size()
    }

    /// Replaces the native options with a newer announcement. A mode selector
    /// that came from legacy session modes or from the client survives an
    /// announcement that does not carry one.
    pub fn adopt(&self, mut next: Self) -> Result<Self, Error> {
        if next.mode().is_none()
            && let Some(mode) = self.mode()
            && let Some(route) = self
                .routes
                .get(&mode.id)
                .filter(|route| **route != Route::ConfigOption)
            && !next.routes.contains_key(&mode.id)
        {
            next.routes.insert(mode.id.clone(), route.clone());
            next.values.push(mode.clone());
        }
        next.check_size()?;
        Ok(next)
    }

    fn check_size(&self) -> Result<(), Error> {
        if self.values.len() > 64
            || serde_json::to_vec(&self.values)?.len() > soyeht_protocol::limits::JSON_BYTES / 2
        {
            return Err(Error::ConfigurationMetadataTooLarge);
        }
        Ok(())
    }

    /// The native request that sets `id`, or `None` when the client enforces
    /// the option itself and may apply it with [`Options::confirmed`] at once.
    pub fn request(
        &self,
        session: &str,
        id: &str,
        value: &ConfigValue,
    ) -> Result<Option<(&'static str, Value)>, Error> {
        let option = self
            .values
            .iter()
            .find(|option| option.id == id)
            .ok_or(Error::UnknownConfigurationOption)?;
        let value = serde_json::to_value(value)?;
        match option.r#type {
            ConfigType::Boolean if value.is_boolean() => (),
            ConfigType::Select
                if option
                    .options
                    .iter()
                    .any(|choice| Some(choice.value.as_str()) == value.as_str()) => {}
            _ => {
                return Err(Error::UnavailableConfigurationChoice);
            }
        }
        match self.routes.get(id) {
            Some(Route::Client) => Ok(None),
            Some(Route::SessionMode) => Ok(Some((
                "session/set_mode",
                json!({"sessionId":session,"modeId":value}),
            ))),
            _ => {
                let mut params = json!({"sessionId":session,"configId":id,"value":value});
                if option.r#type == ConfigType::Boolean {
                    params["type"] = json!("boolean");
                }
                Ok(Some(("session/set_config_option", params)))
            }
        }
    }

    /// Apply only after a successful native response, never when sending the
    /// request. Client-enforced options need no response.
    pub fn confirmed(
        &self,
        id: &str,
        value: &ConfigValue,
        response: &Value,
    ) -> Result<Self, Error> {
        if matches!(
            self.routes.get(id),
            Some(Route::SessionMode | Route::Client)
        ) {
            let mut next = self.clone();
            let option = next
                .values
                .iter_mut()
                .find(|option| option.id == id)
                .ok_or(Error::MissingModeOption)?;
            option.current_value = value.clone();
            return Ok(next);
        }
        if !response["configOptions"].is_array() {
            return Err(Error::MissingConfirmedOptions);
        }
        let next = self.adopt(Self::from_response(response)?)?;
        if !next
            .values
            .iter()
            .any(|option| option.id == id && option.current_value == *value)
        {
            return Err(Error::UnconfirmedConfigurationValue);
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
        let (method, params) = options
            .request("session", "model", &value)
            .unwrap()
            .unwrap();
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
            options
                .request("session", "fast", &boolean)
                .unwrap()
                .unwrap()
                .1["type"],
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
            Some((
                "session/set_mode",
                json!({"sessionId":"session","modeId":"all"})
            ))
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
            next.request("session", "mode", &value).unwrap().unwrap().0,
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

    #[test]
    fn mode_kinds_come_from_metadata_or_well_known_full_access_ids() {
        let options = Options::from_response(
            &json!({"modes":{"currentModeId":"default","availableModes":[
                {"id":"default","name":"Manual","_meta":{"kind":"standard"}},
                {"id":"auto","name":"Auto","_meta":{"kind":"auto_review"}},
                {"id":"bypassPermissions","name":"Bypass permissions"}
            ]}}),
        )
        .unwrap();
        let kinds: Vec<_> = options
            .mode()
            .unwrap()
            .options
            .iter()
            .map(|c| c.kind.as_deref())
            .collect();
        assert_eq!(
            kinds,
            [
                Some("standard"),
                Some("auto_review"),
                Some(FULL_ACCESS_KIND)
            ]
        );
        assert_eq!(options.current_mode(), Some("default"));
        assert!(options.offers_mode("auto") && !options.offers_mode("plan"));
        let other = Options::from_response(&json!({"configOptions":[{"id":"agent","name":"Agent","category":"agent","type":"select","currentValue":"bypassPermissions","options":[{"value":"bypassPermissions","name":"Not a mode"}]}]})).unwrap();
        assert_eq!(other.values[0].options[0].kind, None);
    }

    #[test]
    fn client_modes_apply_without_a_native_request_and_survive_announcements() {
        let mut options = Options::from_response(&json!({"configOptions":[{"id":"fast","name":"Fast","type":"boolean","currentValue":false}]})).unwrap();
        options
            .ensure_mode(|| ConfigOption {
                id: "mode".into(),
                name: "Mode".into(),
                description: None,
                category: Some(PERMISSION_MODE_CATEGORY.into()),
                r#type: ConfigType::Select,
                current_value: ConfigValue::V0("ask".into()),
                options: vec![
                    ConfigChoice {
                        value: "ask".into(),
                        name: "Ask".into(),
                        description: None,
                        kind: None,
                    },
                    ConfigChoice {
                        value: "all".into(),
                        name: "All".into(),
                        description: None,
                        kind: Some(FULL_ACCESS_KIND.into()),
                    },
                ],
            })
            .unwrap();
        let all = ConfigValue::V0("all".into());
        assert_eq!(options.request("session", "mode", &all).unwrap(), None);
        let options = options.confirmed("mode", &all, &Value::Null).unwrap();
        assert_eq!(options.current_mode(), Some("all"));
        let announced = options.adopt(Options::from_response(&json!({"configOptions":[{"id":"fast","name":"Fast","type":"boolean","currentValue":true}]})).unwrap()).unwrap();
        assert_eq!(announced.current_mode(), Some("all"));
        assert_eq!(announced.route("mode"), Some(&Route::Client));
        let native = announced.adopt(Options::from_response(&json!({"configOptions":[{"id":"mode","name":"Mode","category":"mode","type":"select","currentValue":"x","options":[{"value":"x","name":"X"}]}]})).unwrap()).unwrap();
        assert_eq!(native.route("mode"), Some(&Route::ConfigOption));
        assert_eq!(native.current_mode(), Some("x"));
    }
}
