//! The questions an agent asks: a form made of the properties of the request's schema.

use serde_json::Value;

pub const CANCEL: &str = "Cancel";
pub const SUBMIT: &str = "Submit";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub key: String,
    /// The property's title, or its key when it has none.
    pub title: String,
}

/// The fields of a request's schema, sorted by key. A schema without properties has none, and
/// then there is nothing to submit.
pub fn fields(schema: &Value) -> Vec<Field> {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut fields: Vec<Field> = properties
        .iter()
        .map(|(key, property)| Field {
            key: key.clone(),
            title: property
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(key)
                .to_owned(),
        })
        .collect();
    fields.sort_by(|left, right| left.key.cmp(&right.key));
    fields
}

/// What an answer carries: the text of each field the person typed in. A field left empty is not
/// sent.
pub fn answer(typed: impl IntoIterator<Item = (String, String)>) -> Value {
    Value::Object(
        typed
            .into_iter()
            .filter(|(_, text)| !text.is_empty())
            .map(|(key, text)| (key, Value::String(text)))
            .collect(),
    )
}
