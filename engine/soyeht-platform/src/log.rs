//! One structured log line: a JSON object on standard error.
//!
//! Every crate that logs a failure nothing else reports uses this, so no log line is
//! assembled by hand and every value is escaped the same way. A line that is built where
//! it cannot be written (under a lock, or for a test to read) is a [`Line`].

use std::{
    fmt::Display,
    io::{self, Write},
};

/// Writes `{"event":name, ...fields}` as one line to standard error.
pub fn event(name: &str, fields: &[(&str, &str)]) {
    write_line(&record(name, fields));
}

/// One log line that is built now and written later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    name: String,
    fields: Vec<(String, String)>,
}

impl Line {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            fields: Vec::new(),
        }
    }

    /// Adds a field. Every value is logged as text.
    #[must_use]
    pub fn field(mut self, key: &str, value: impl Display) -> Self {
        self.fields.push((key.to_owned(), value.to_string()));
        self
    }

    /// Adds a field when there is a value; a line never carries an empty field.
    #[must_use]
    pub fn field_opt(self, key: &str, value: Option<impl Display>) -> Self {
        match value {
            Some(value) => self.field(key, value),
            None => self,
        }
    }

    /// The JSON object of the line, without its newline.
    pub fn json(&self) -> String {
        let fields: Vec<(&str, &str)> = self
            .fields
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        record(&self.name, &fields)
    }

    pub fn emit(&self) {
        write_line(&self.json());
    }
}

/// A failure that logs as a bounded label, never as message text: the message of an I/O error
/// can carry a path, and a log line is kept long after the request that caused it.
pub trait Failure {
    /// The label the line carries in its `kind` field.
    fn failure_kind(&self) -> String;
}

impl Failure for io::Error {
    fn failure_kind(&self) -> String {
        format!("{:?}", self.kind())
    }
}

/// Writes `event` with the kind of `error` as its `kind` field.
pub fn failure(name: &str, error: &dyn Failure) {
    failure_with(name, &[], error);
}

/// [`failure`] with more fields before `kind`.
pub fn failure_with(name: &str, fields: &[(&str, &str)], error: &dyn Failure) {
    write_line(&failure_record(name, fields, error));
}

fn failure_record(name: &str, fields: &[(&str, &str)], error: &dyn Failure) -> String {
    let kind = error.failure_kind();
    let mut all = fields.to_vec();
    all.push(("kind", &kind));
    record(name, &all)
}

/// Writes one line to standard error. `eprintln!` panics when standard error is closed, and a
/// log line must never stop the program that writes it.
fn write_line(line: &str) {
    // A closed standard error (a broken pipe) has nowhere to report itself.
    let _ = writeln!(io::stderr(), "{line}");
}

/// The JSON object of a log line, without its newline.
pub fn record(name: &str, fields: &[(&str, &str)]) -> String {
    let mut line = String::from("{");
    for (index, (key, value)) in std::iter::once(&("event", name)).chain(fields).enumerate() {
        if index > 0 {
            line.push(',');
        }
        push_string(&mut line, key);
        line.push(':');
        push_string(&mut line, value);
    }
    line.push('}');
    line
}

fn push_string(line: &mut String, text: &str) {
    line.push('"');
    for character in text.chars() {
        match character {
            '"' => line.push_str("\\\""),
            '\\' => line.push_str("\\\\"),
            '\n' => line.push_str("\\n"),
            '\r' => line.push_str("\\r"),
            '\t' => line.push_str("\\t"),
            control if control < ' ' => line.push_str(&format!("\\u{:04x}", u32::from(control))),
            other => line.push(other),
        }
    }
    line.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_one_json_object_with_the_event_first() {
        assert_eq!(record("ptyd.ready", &[]), r#"{"event":"ptyd.ready"}"#);
        assert_eq!(
            record("process.failed", &[("error", "NotFound"), ("pid", "7")]),
            r#"{"event":"process.failed","error":"NotFound","pid":"7"}"#
        );
    }

    struct Refused;

    impl Failure for Refused {
        fn failure_kind(&self) -> String {
            "refused".to_owned()
        }
    }

    #[test]
    fn a_failure_line_carries_the_kind_last_and_never_the_message() {
        let error = io::Error::new(io::ErrorKind::NotFound, "/private/path/secret");
        let line = failure_record("ptyd.child_wait_failed", &[], &error);
        assert_eq!(
            line,
            r#"{"event":"ptyd.child_wait_failed","kind":"NotFound"}"#
        );
        assert!(!line.contains("secret"));
        assert_eq!(
            failure_record("engine.disconnect", &[("connectionId", "c1")], &Refused),
            r#"{"event":"engine.disconnect","connectionId":"c1","kind":"refused"}"#
        );
    }

    #[test]
    fn a_line_built_now_is_written_with_its_fields_in_order() {
        let line = Line::new("engine.editor_effect_failed")
            .field("action", "save")
            .field("attempts", 4);
        assert_eq!(
            line.json(),
            r#"{"event":"engine.editor_effect_failed","action":"save","attempts":"4"}"#
        );
    }

    #[test]
    fn quotes_backslashes_and_control_characters_are_escaped() {
        let line = record("e", &[("path", "a\"b\\c\nd\u{1}")]);
        assert_eq!(
            line,
            "{\"event\":\"e\",\"path\":\"a\\\"b\\\\c\\nd\\u0001\"}"
        );
    }
}
