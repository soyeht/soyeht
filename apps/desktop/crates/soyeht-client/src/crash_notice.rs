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
    pending: usize,
}

/// Whether a report is waiting for the person. Blocks until the program ends: run it off the
/// window's thread.
pub fn pending() -> Result<bool, Failure> {
    parse(&ask(&["crash-reports", "status"])?)
}

fn parse(text: &str) -> Result<bool, Failure> {
    serde_json::from_str::<Status>(text)
        .map(|status| status.pending > 0)
        .map_err(|_| Failure::NotText)
}

/// Marks the reports as seen: the notice does not come back for them.
pub fn acknowledge() -> Result<(), Failure> {
    ask(&["crash-reports", "acknowledge"]).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_count_above_zero_is_a_notice() {
        assert_eq!(
            parse(r#"{"pending":2,"latest":{"program":"engine","time":5}}"#),
            Ok(true)
        );
        assert_eq!(parse(r#"{"pending":0,"latest":null}"#), Ok(false));
    }

    #[test]
    fn an_answer_that_is_not_the_status_is_a_failure_not_a_notice() {
        assert_eq!(parse("not json"), Err(Failure::NotText));
        assert_eq!(parse("{}"), Err(Failure::NotText));
    }
}
