//! Crash reports (contract 16, ADR 0127): a small local file a program writes when it panics,
//! that a person can send on purpose. Nothing is uploaded.
//!
//! [`install`] puts the hook in a program. [`store`] keeps the reports in `crashes/` of the data
//! directory, bounded and oldest first out, and says which ones the person has not seen.
//! `diagnostics` pastes them into the report of `soyeht diagnostics` ([`markdown`]).

mod capture;
mod frames;
mod report;
pub mod store;
#[cfg(test)]
mod tests;

pub use capture::{Settings, install};
pub use report::{
    LINE_CHARS, MAGIC, MAX_FRAMES, MAX_LOG_LINES, MAX_REPORT_BYTES, MAX_SYMBOLS, Program, Report,
    utc,
};

use crate::diagnostics::Redactor;

/// `reports` as the Markdown the diagnostics report holds: one block each, newest last, with the
/// log lines passed through `redactor` once more (a file in the folder may not be ours).
pub fn markdown(reports: &[store::Stored], redactor: &Redactor) -> Vec<String> {
    let mut lines = Vec::new();
    for stored in reports {
        let report = &stored.report;
        lines.extend([
            String::new(),
            format!(
                "#### {} · {}{}",
                report.program,
                utc(report.time),
                if report.fatal { " · closed" } else { "" }
            ),
            String::new(),
            "```".to_owned(),
        ]);
        let home = |text: &str| redactor.strip_home(text);
        let text = Report {
            version: home(&report.version),
            system: home(&report.system),
            thread: home(&report.thread),
            kind: home(&report.kind),
            location: home(&report.location),
            frames: report
                .frames
                .iter()
                .map(|line| redactor.strip_home(line))
                .collect(),
            symbols: report
                .symbols
                .iter()
                .map(|line| redactor.strip_home(line))
                .collect(),
            log: report
                .log
                .iter()
                .map(|line| redactor.redact(line))
                .collect(),
            ..report.clone()
        }
        .to_text();
        lines.extend(text.lines().map(str::to_owned));
        lines.push("```".to_owned());
    }
    lines
}
