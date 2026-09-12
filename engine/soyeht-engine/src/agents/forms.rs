use serde_json::Value;

fn allowed_key(key: &str) -> bool {
    matches!(
        key,
        "type"
            | "title"
            | "description"
            | "properties"
            | "required"
            | "additionalProperties"
            | "enum"
            | "enumNames"
            | "const"
            | "items"
            | "minItems"
            | "maxItems"
            | "minLength"
            | "maxLength"
            | "minimum"
            | "maximum"
            | "default"
            | "format"
            | "$schema"
            | "oneOf"
            | "anyOf"
    )
}

/// Unsupported assertions are refused before displaying the question. They
/// are never silently ignored while validating a submitted answer.
pub fn supported(schema: &Value, depth: usize) -> bool {
    let Some(object) = schema.as_object() else {
        return false;
    };
    if depth > 8 || object.keys().any(|key| !allowed_key(key)) {
        return false;
    }
    if schema.get("type").is_some_and(|kind| {
        !matches!(
            kind.as_str(),
            Some("object" | "array" | "string" | "boolean" | "null" | "number" | "integer")
        )
    }) {
        return false;
    }
    for name in ["required", "enumNames"] {
        if schema.get(name).is_some_and(|value| {
            !value
                .as_array()
                .is_some_and(|items| items.len() <= 128 && items.iter().all(Value::is_string))
        }) {
            return false;
        }
    }
    for name in ["minItems", "maxItems", "minLength", "maxLength"] {
        if schema
            .get(name)
            .is_some_and(|value| value.as_u64().is_none())
        {
            return false;
        }
    }
    for name in ["minimum", "maximum"] {
        if schema.get(name).is_some_and(|value| !value.is_number()) {
            return false;
        }
    }
    if schema.get("enum").is_some_and(|value| {
        !value
            .as_array()
            .is_some_and(|items| !items.is_empty() && items.len() <= 128)
    }) {
        return false;
    }
    // These assertions depend on a concrete type; accepting them without one
    // would otherwise skip validation in the value dispatcher below.
    for (kind, keys) in [
        (
            "object",
            &["properties", "required", "additionalProperties"][..],
        ),
        ("array", &["items", "minItems", "maxItems"][..]),
        ("string", &["minLength", "maxLength"][..]),
    ] {
        if keys.iter().any(|key| schema.get(key).is_some()) && schema["type"] != kind {
            return false;
        }
    }
    if ["minimum", "maximum"]
        .iter()
        .any(|key| schema.get(key).is_some())
        && !matches!(schema["type"].as_str(), Some("number" | "integer"))
    {
        return false;
    }
    if let Some(properties) = schema.get("properties") {
        let Some(properties) = properties.as_object() else {
            return false;
        };
        if properties.len() > 128
            || properties
                .values()
                .any(|value| !supported(value, depth + 1))
        {
            return false;
        }
    }
    if let Some(items) = schema.get("items")
        && !supported(items, depth + 1)
    {
        return false;
    }
    for kind in ["oneOf", "anyOf"] {
        if let Some(branches) = schema.get(kind) {
            let Some(branches) = branches.as_array() else {
                return false;
            };
            if branches.is_empty()
                || branches.len() > 128
                || branches.iter().any(|branch| !supported(branch, depth + 1))
            {
                return false;
            }
        }
    }
    schema
        .get("additionalProperties")
        .is_none_or(Value::is_boolean)
}

pub fn valid(schema: &Value, value: &Value) -> bool {
    if !supported(schema, 0)
        || !serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= 512 * 1024)
    {
        return false;
    }
    matches_schema(schema, value)
}

fn matches_schema(schema: &Value, value: &Value) -> bool {
    if let Some(constant) = schema.get("const")
        && constant != value
    {
        return false;
    }
    if let Some(choices) = schema.get("enum")
        && !choices
            .as_array()
            .is_some_and(|choices| choices.contains(value))
    {
        return false;
    }
    if let Some(branches) = schema.get("oneOf").and_then(Value::as_array)
        && branches
            .iter()
            .filter(|branch| matches_schema(branch, value))
            .count()
            != 1
    {
        return false;
    }
    if let Some(branches) = schema.get("anyOf").and_then(Value::as_array)
        && !branches.iter().any(|branch| matches_schema(branch, value))
    {
        return false;
    }
    match schema.get("type").and_then(Value::as_str) {
        None => true,
        Some("object") => {
            let Some(object) = value.as_object() else {
                return false;
            };
            if object.len() > 128 {
                return false;
            }
            if schema
                .get("required")
                .and_then(Value::as_array)
                .is_some_and(|required| {
                    required
                        .iter()
                        .any(|key| !key.as_str().is_some_and(|key| object.contains_key(key)))
                })
            {
                return false;
            }
            object.iter().all(|(key, value)| {
                match schema.get("properties").and_then(|p| p.get(key)) {
                    Some(rule) => matches_schema(rule, value),
                    None => schema.get("additionalProperties") != Some(&Value::Bool(false)),
                }
            })
        }
        Some("array") => {
            let Some(items) = value.as_array() else {
                return false;
            };
            items.len() <= 256
                && count(schema, "minItems", "maxItems", items.len())
                && schema
                    .get("items")
                    .is_none_or(|rule| items.iter().all(|item| matches_schema(rule, item)))
        }
        Some("string") => value
            .as_str()
            .is_some_and(|text| count(schema, "minLength", "maxLength", text.chars().count())),
        Some("boolean") => value.is_boolean(),
        Some("null") => value.is_null(),
        Some(kind @ ("number" | "integer")) => value.as_f64().is_some_and(|number| {
            (kind == "number" || number.fract() == 0.0)
                && schema
                    .get("minimum")
                    .and_then(Value::as_f64)
                    .is_none_or(|minimum| number >= minimum)
                && schema
                    .get("maximum")
                    .and_then(Value::as_f64)
                    .is_none_or(|maximum| number <= maximum)
        }),
        Some(_) => false,
    }
}
fn count(schema: &Value, min: &str, max: &str, length: usize) -> bool {
    schema
        .get(min)
        .and_then(Value::as_u64)
        .is_none_or(|n| length as u64 >= n)
        && schema
            .get(max)
            .and_then(Value::as_u64)
            .is_none_or(|n| length as u64 <= n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn required_fields_choices_bounds_and_unsupported_assertions_are_checked() {
        let schema = json!({"type":"object","additionalProperties":false,"required":["name","count","approve"],"properties":{"name":{"type":"string","minLength":2,"maxLength":5},"count":{"type":"integer","minimum":1,"maximum":4},"approve":{"type":"boolean"},"color":{"type":"string","oneOf":[{"const":"red","title":"Red"},{"const":"blue","title":"Blue"}]}}});
        let good = json!({"name":"Ada","count":2,"approve":true,"color":"red"});
        assert!(valid(&schema, &good));
        for (key, value) in [
            ("name", json!("x")),
            ("count", json!(1.5)),
            ("count", json!(5)),
            ("approve", json!("yes")),
            ("color", json!("green")),
            ("extra", json!(true)),
        ] {
            let mut bad = good.clone();
            bad[key] = value;
            assert!(!valid(&schema, &bad));
        }
        assert!(!valid(&schema, &json!({"name":"Ada"})));
        assert!(!supported(&json!({"type":"string","pattern":"^abc$"}), 0));
        for malformed in [
            json!({"type":[]}),
            json!({"required":[]}),
            json!({"type":"object","required":true}),
            json!({"type":"string","minLength":-1}),
            json!({"type":"integer","minimum":"1"}),
        ] {
            assert!(!supported(&malformed, 0));
        }
    }
}
