//! Checks a JSON value against the generated model of its schema (ADR 0090).
//!
//! The engine builds events and results as JSON. With the `contract-checks` feature on, every
//! event and every result is decoded into the generated type of its schema and encoded again.
//! A value fails when the type refuses it (a missing or mistyped field, an unknown enum value)
//! or when the round trip drops something the engine wrote (a field the contract does not
//! have, for instance a misspelt one). No shipping build enables the feature.

use crate::{EVENTS, METHODS};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::fmt;

mod roundtrip;

/// Why a value does not follow its schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mismatch {
    /// The name is not a schema, an event or a method of the contract.
    Unknown(String),
    /// The value breaks the schema; `reason` says where.
    Invalid { schema: String, reason: String },
}

impl fmt::Display for Mismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(name) => write!(formatter, "{name} is not in the contract"),
            Self::Invalid { schema, reason } => write!(formatter, "{schema}: {reason}"),
        }
    }
}

impl std::error::Error for Mismatch {}

pub(crate) fn decode_encode<T: Serialize + DeserializeOwned>(
    schema: &str,
    value: &Value,
) -> Result<Value, Mismatch> {
    let typed = serde_json::from_value::<T>(value.clone()).map_err(|error| Mismatch::Invalid {
        schema: schema.to_owned(),
        reason: format!("{error} in {}", excerpt(value)),
    })?;
    serde_json::to_value(typed).map_err(|error| Mismatch::Invalid {
        schema: schema.to_owned(),
        reason: error.to_string(),
    })
}

/// The start of `value`, so a failure names what was sent. The checks run in tests only.
fn excerpt(value: &Value) -> String {
    value.to_string().chars().take(400).collect()
}

/// Decodes `value` as the schema `name` and encodes it again.
pub fn roundtrip(name: &str, value: &Value) -> Result<Value, Mismatch> {
    roundtrip::roundtrip(name, value)
}

/// Checks `value` against the schema `name`.
pub fn validate(name: &str, value: &Value) -> Result<(), Mismatch> {
    let encoded = roundtrip(name, value)?;
    let mut lost = Vec::new();
    differences(value, &encoded, String::new(), &mut lost);
    if lost.is_empty() {
        Ok(())
    } else {
        Err(Mismatch::Invalid {
            schema: name.to_owned(),
            reason: lost.join("; "),
        })
    }
}

/// Checks the parameters of the event `name`.
pub fn event(name: &str, params: &Value) -> Result<(), Mismatch> {
    let descriptor = EVENTS
        .iter()
        .find(|event| event.name.as_str() == name)
        .ok_or_else(|| Mismatch::Unknown(name.to_owned()))?;
    validate(descriptor.params_type, params).map_err(|error| in_context(name, error))
}

/// Checks the result of the method `name`.
pub fn result(name: &str, value: &Value) -> Result<(), Mismatch> {
    let (_, _, result_type, _) = METHODS
        .iter()
        .find(|(method, ..)| *method == name)
        .ok_or_else(|| Mismatch::Unknown(name.to_owned()))?;
    validate(result_type, value).map_err(|error| in_context(name, error))
}

fn in_context(name: &str, error: Mismatch) -> Mismatch {
    match error {
        Mismatch::Invalid { schema, reason } => Mismatch::Invalid {
            schema: format!("{name} ({schema})"),
            reason,
        },
        unknown => unknown,
    }
}

/// Lists what `before` has and `after` lost or changed.
fn differences(before: &Value, after: &Value, path: String, lost: &mut Vec<String>) {
    match (before, after) {
        (Value::Object(before), Value::Object(after)) => {
            for (key, value) in before {
                let at = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                match after.get(key) {
                    Some(encoded) => differences(value, encoded, at, lost),
                    None if value.is_null() => {
                        lost.push(format!("{at} is null for an optional field; leave it out"));
                    }
                    None => lost.push(format!("{at} is not in the contract")),
                }
            }
        }
        (Value::Array(before), Value::Array(after)) if before.len() == after.len() => {
            for (index, (value, encoded)) in before.iter().zip(after).enumerate() {
                differences(value, encoded, format!("{path}[{index}]"), lost);
            }
        }
        // 1 and 1.0 are the same number to a client; an integer literal in a float field
        // comes back from the round trip as 1.0.
        (Value::Number(before), Value::Number(after)) if before.as_f64() == after.as_f64() => (),
        _ if before != after => lost.push(format!("{path} changed in the round trip")),
        _ => (),
    }
}
