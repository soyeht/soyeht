//! The script world of the reader on the systems whose web view has no world of its own to hand
//! out (ADR 0124): what a script is run with, and what it answers, as text. The calls themselves
//! are in `webview2/script.rs`; this is the arithmetic of their JSON, which is the same on every
//! system and is tested on each.

use crate::{Error, Result};
use serde_json::{Value, json};

/// The calls of the engine's protocol the reader makes, and no others (ADR 0124). A call is made
/// from this list, so no name is built from text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Method {
    FrameTree,
    CreateWorld,
    Evaluate,
    ReloadIgnoringCache,
}

impl Method {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 4] = [
        Self::FrameTree,
        Self::CreateWorld,
        Self::Evaluate,
        Self::ReloadIgnoringCache,
    ];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::FrameTree => "Page.getFrameTree",
            Self::CreateWorld => "Page.createIsolatedWorld",
            Self::Evaluate => "Runtime.evaluate",
            Self::ReloadIgnoringCache => "Page.reload",
        }
    }
}

/// The arguments of [`Method::ReloadIgnoringCache`].
pub(crate) const RELOAD_ARGUMENTS: &str = r#"{"ignoreCache":true}"#;

/// The name of the world every script of the reader runs in. A page has no handle on it.
pub(crate) const WORLD_NAME: &str = "soyeht-read";

/// How long the page's engine may run a script, in milliseconds. The caller's own wait is longer.
const SCRIPT_BUDGET_MS: u32 = 15_000;

/// The arguments that make the world, in the frame whose id `frame` is.
pub(crate) fn create_world_arguments(frame: &str) -> String {
    json!({ "frameId": frame, "worldName": WORLD_NAME, "grantUniveralAccess": false }).to_string()
}

/// The arguments that run `source` in the world whose context is `context`. A value comes back
/// as data (`returnByValue`) after a promise, if it is one, has settled.
pub(crate) fn evaluate_arguments(context: i64, source: &str) -> String {
    json!({
        "expression": source,
        "contextId": context,
        "returnByValue": true,
        "awaitPromise": true,
        "silent": true,
        "userGesture": false,
        "timeout": SCRIPT_BUDGET_MS,
    })
    .to_string()
}

/// The id of the page's top frame, from the answer about the frame tree.
pub(crate) fn frame_id(answer: &str) -> Result<String> {
    let value: Value = parse(answer)?;
    value
        .pointer("/frameTree/frame/id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(Error::Page("the page has no frame"))
}

/// The context of the world, from the answer to making it.
pub(crate) fn context_id(answer: &str) -> Result<i64> {
    parse(answer)?
        .get("executionContextId")
        .and_then(Value::as_i64)
        .ok_or(Error::Page("the script world could not be made"))
}

/// What the script returned, as text: a string as it is, anything else as the page would write
/// it (`true`, `42`, `null`, `undefined`), a script that threw as an error.
pub(crate) fn value_text(answer: &str) -> Result<String> {
    let value = parse(answer)?;
    if value.get("exceptionDetails").is_some() {
        return Err(Error::Page("the script did not finish"));
    }
    let result = value
        .get("result")
        .ok_or(Error::Page("the script did not answer"))?;
    match result.get("value") {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(other) => Ok(other.to_string()),
        None => match result.get("type").and_then(Value::as_str) {
            Some("undefined") => Ok("undefined".to_owned()),
            _ => Err(Error::Page("the script returned nothing to read")),
        },
    }
}

fn parse(answer: &str) -> Result<Value> {
    serde_json::from_str(answer).map_err(|_| Error::Page("the answer could not be read"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_calls_of_the_reader_are_four_and_none_enables_a_domain() {
        let names: Vec<&str> = Method::ALL.iter().map(|method| method.name()).collect();
        assert_eq!(
            names,
            [
                "Page.getFrameTree",
                "Page.createIsolatedWorld",
                "Runtime.evaluate",
                "Page.reload"
            ]
        );
        assert!(names.iter().all(|name| !name.ends_with(".enable")));
    }

    #[test]
    fn a_string_comes_back_as_it_is_and_other_values_as_the_page_writes_them() {
        let text = |json: &str| value_text(json).unwrap();
        assert_eq!(
            text(r#"{"result":{"type":"string","value":"a\"b"}}"#),
            "a\"b"
        );
        assert_eq!(
            text(r#"{"result":{"type":"boolean","value":true}}"#),
            "true"
        );
        assert_eq!(text(r#"{"result":{"type":"number","value":42}}"#), "42");
        assert_eq!(
            text(r#"{"result":{"type":"object","subtype":"null","value":null}}"#),
            "null"
        );
        assert_eq!(text(r#"{"result":{"type":"undefined"}}"#), "undefined");
    }

    #[test]
    fn a_script_that_threw_or_an_answer_that_is_not_one_is_an_error() {
        assert!(
            value_text(r#"{"result":{"type":"object"},"exceptionDetails":{"text":"x"}}"#).is_err()
        );
        assert!(value_text("not json").is_err());
        assert!(value_text("{}").is_err());
        assert!(value_text(r#"{"result":{"type":"object"}}"#).is_err());
    }

    #[test]
    fn the_frame_and_the_context_are_read_from_their_answers() {
        assert_eq!(
            frame_id(r#"{"frameTree":{"frame":{"id":"F1","url":"x"}}}"#).unwrap(),
            "F1"
        );
        assert!(frame_id(r#"{"frameTree":{}}"#).is_err());
        assert_eq!(context_id(r#"{"executionContextId":7}"#).unwrap(), 7);
        assert!(context_id("{}").is_err());
    }

    #[test]
    fn a_script_is_run_in_the_world_and_gives_a_value_after_its_promise() {
        let arguments: Value = serde_json::from_str(&evaluate_arguments(3, "1+1")).unwrap();
        assert_eq!(arguments["contextId"], 3);
        assert_eq!(arguments["expression"], "1+1");
        assert_eq!(arguments["returnByValue"], true);
        assert_eq!(arguments["awaitPromise"], true);
        let world: Value = serde_json::from_str(&create_world_arguments("F1")).unwrap();
        assert_eq!(world["worldName"], WORLD_NAME);
        assert_eq!(world["grantUniveralAccess"], false);
    }
}
