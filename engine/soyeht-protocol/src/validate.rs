use crate::{Request, SCHEMA};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;
use std::sync::OnceLock;

fn schema() -> Result<&'static Value, &'static str> {
    static PARSED: OnceLock<Result<Value, String>> = OnceLock::new();
    PARSED
        .get_or_init(|| serde_json::from_str(SCHEMA).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|_| "compiled schema is invalid")
}

pub fn parameters(method: &str, value: &Value) -> Result<Request, &'static str> {
    let schema = schema()?;
    let registry = &schema["x-methods"];
    let definition = registry.get(method).ok_or("unknown method")?;
    if !valid(schema, &definition["params"], value) {
        return Err("invalid parameters");
    }
    serde_json::from_value(serde_json::json!({"method":method,"params":value}))
        .map_err(|_| "invalid parameters")
}

fn valid(schema: &Value, rule: &Value, value: &Value) -> bool {
    if let Some(reference) = rule.get("$ref").and_then(Value::as_str) {
        let Some(target) = reference
            .strip_prefix("#/$defs/")
            .and_then(|name| schema["$defs"].get(name))
        else {
            return false;
        };
        return valid(schema, target, value);
    }
    if let Some(branches) = rule.get("oneOf").and_then(Value::as_array) {
        return branches
            .iter()
            .filter(|branch| valid(schema, branch, value))
            .count()
            == 1;
    }
    if rule
        .get("enum")
        .and_then(Value::as_array)
        .is_some_and(|options| !options.contains(value))
    {
        return false;
    }
    match rule.get("type").and_then(Value::as_str) {
        None => true,
        Some("null") => value.is_null(),
        Some("boolean") => value.is_boolean(),
        Some("string") => {
            let Some(text) = value.as_str() else {
                return false;
            };
            let length = text.chars().count() as u64;
            if rule
                .get("minLength")
                .and_then(Value::as_u64)
                .is_some_and(|minimum| length < minimum)
                || rule
                    .get("maxLength")
                    .and_then(Value::as_u64)
                    .is_some_and(|maximum| length > maximum)
            {
                return false;
            }
            if let Some(pattern) = rule.get("pattern").and_then(Value::as_str)
                && (pattern != "^(0|[1-9][0-9]*)$"
                    || !text.bytes().all(|byte| byte.is_ascii_digit())
                    || text.parse::<u64>().is_err()
                    || (text.len() > 1 && text.starts_with('0')))
            {
                return false;
            }
            rule.get("contentEncoding").and_then(Value::as_str) != Some("base64")
                || STANDARD.decode(text).is_ok()
        }
        Some(kind @ ("integer" | "number")) => {
            if kind == "integer" && !value.is_i64() && !value.is_u64() {
                return false;
            }
            let Some(number) = value.as_f64() else {
                return false;
            };
            !rule
                .get("minimum")
                .and_then(Value::as_f64)
                .is_some_and(|minimum| number < minimum)
                && !rule
                    .get("maximum")
                    .and_then(Value::as_f64)
                    .is_some_and(|maximum| number > maximum)
        }
        Some("array") => value
            .as_array()
            .is_some_and(|items| items.iter().all(|item| valid(schema, &rule["items"], item))),
        Some("object") => {
            let Some(object) = value.as_object() else {
                return false;
            };
            if rule["required"].as_array().is_some_and(|keys| {
                keys.iter()
                    .any(|key| key.as_str().is_none_or(|key| !object.contains_key(key)))
            }) {
                return false;
            }
            rule["properties"].as_object().is_some_and(|properties| {
                properties.iter().all(|(key, child)| {
                    object
                        .get(key)
                        .is_none_or(|value| valid(schema, child, value))
                })
            })
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_bounds_and_types_before_dispatch() {
        let mut params = json!({"paneId":"pane-1","cols":80,"rows":24,"idempotencyKey":"key-1"});
        assert!(parameters("pane.resize", &params).is_ok());
        for cols in [json!(0), json!(1001), json!("80"), json!(-1)] {
            params["cols"] = cols;
            assert!(parameters("pane.resize", &params).is_err());
        }
        assert!(parameters("pane.close", &json!({"paneId":"pane-1"})).is_err());
        assert!(parameters("events.subscribe", &json!({"since":"18446744073709551616"})).is_err());
        assert!(parameters("events.subscribe", &json!({"since":"+1"})).is_err());
        assert!(parameters("events.subscribe", &json!({"since":"01"})).is_err());
    }
}
