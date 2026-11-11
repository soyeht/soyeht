//! The questions an agent asks: a form made of the properties of the request's schema.

use crate::Command;
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

/// The request that answers the question `request_id` with what the person typed (`typed`, a key
/// and its text for each field), or that declines it when `typed` is `None`.
pub fn response(
    pane_id: &str,
    request_id: &str,
    typed: Option<impl IntoIterator<Item = (String, String)>>,
) -> Command {
    let fields = typed.map(answer);
    Command::AnswerElicitation {
        pane_id: pane_id.to_owned(),
        request_id: request_id.to_owned(),
        cancel: fields.is_none(),
        fields,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn typed(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(key, text)| ((*key).to_owned(), (*text).to_owned()))
            .collect()
    }

    #[test]
    fn submitting_answers_with_the_fields_that_have_text() {
        let command = response("p", "r", Some(typed(&[("name", "Ada"), ("note", "")])));
        assert_eq!(
            command,
            Command::AnswerElicitation {
                pane_id: "p".into(),
                request_id: "r".into(),
                fields: Some(json!({"name": "Ada"})),
                cancel: false,
            }
        );
    }

    #[test]
    fn declining_cancels_and_carries_no_fields() {
        let command = response("p", "r", None::<Vec<(String, String)>>);
        assert_eq!(
            command,
            Command::AnswerElicitation {
                pane_id: "p".into(),
                request_id: "r".into(),
                fields: None,
                cancel: true,
            }
        );
    }
}
