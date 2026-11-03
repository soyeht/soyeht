//! The notice "Soyeht closed unexpectedly" (ADR 0127, contract 16): whether the crash reports of
//! the profile hold one the person has not seen, and the acknowledgement once they have.
//!
//! The answer is the `soyeht crash-reports` program's, run as the diagnostics are
//! ([`crate::diagnostics::ask`]); the window keeps no rule about which report counts. "Copy
//! report" is "Copy diagnostics", which carries the reports.

use crate::diagnostics::{Failure, ask};
use serde::Deserialize;

/// What `soyeht crash-reports status` prints.
#[derive(Deserialize)]
struct Status {
    reports: Vec<String>,
}

/// The names of the reports waiting for the person, none when there is nothing to tell. Blocks
/// until the program ends: run it off the window's thread.
pub fn pending() -> Result<Vec<String>, Failure> {
    parse(&ask(&["crash-reports", "status"])?)
}

fn parse(text: &str) -> Result<Vec<String>, Failure> {
    serde_json::from_str::<Status>(text)
        .map(|status| status.reports)
        .map_err(|_| Failure::NotText)
}

/// Marks the reports the notice showed as seen: the notice does not come back for them, and a
/// report that came after it was drawn is still pending.
pub fn acknowledge(shown: &[String]) -> Result<(), Failure> {
    let mut words = vec!["crash-reports", "acknowledge"];
    words.extend(shown.iter().map(String::as_str));
    ask(&words).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_of_the_pending_reports_are_the_notice() {
        assert_eq!(
            parse(r#"{"pending":2,"reports":["a.txt","b.txt"],"latest":null}"#),
            Ok(vec!["a.txt".to_owned(), "b.txt".to_owned()])
        );
        assert_eq!(
            parse(r#"{"pending":0,"reports":[],"latest":null}"#),
            Ok(vec![])
        );
    }

    #[test]
    fn an_answer_that_is_not_the_status_is_a_failure_not_a_notice() {
        assert_eq!(parse("not json"), Err(Failure::NotText));
        assert_eq!(parse("{}"), Err(Failure::NotText));
    }
}
