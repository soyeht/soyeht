//! One structured log line: a JSON object on standard error.
//!
//! Every crate that logs a failure nothing else reports uses this, so no log line is
//! assembled by hand and every value is escaped the same way.

use std::io;

/// Writes `{"event":name, ...fields}` as one line to standard error.
pub fn event(name: &str, fields: &[(&str, &str)]) {
    eprintln!("{}", record(name, fields));
}

/// Writes `event` with the kind of `error` as its `error` field. The message of an I/O error
/// can carry a path, so only its kind is logged.
pub fn io_failure(name: &str, error: &io::Error) {
    event(name, &[("error", &format!("{:?}", error.kind()))]);
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

    #[test]
    fn quotes_backslashes_and_control_characters_are_escaped() {
        let line = record("e", &[("path", "a\"b\\c\nd\u{1}")]);
        assert_eq!(
            line,
            "{\"event\":\"e\",\"path\":\"a\\\"b\\\\c\\nd\\u0001\"}"
        );
    }
}
