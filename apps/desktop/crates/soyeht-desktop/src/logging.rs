//! The client's log file.
//!
//! A release client on Windows is a window with no console, and a Linux client started from a
//! menu has no terminal, so a line written to standard error is lost, including the advice about
//! a missing driver. Every line is also written to a bounded file in the profile's `logs`
//! directory (`soyeht_platform::log::to_file`).

use soyeht_client::profile;
use soyeht_platform::{dialog, log};
use std::path::Path;

/// Starts writing log lines to the profile's log file, and logs where it is. A client that
/// cannot open the file keeps logging to standard error alone.
pub(crate) fn start() {
    // No home directory means no profile directory: `run` reports that same cause and stops.
    let Ok(path) = profile::log_file() else {
        return;
    };
    start_at(&path);
}

fn start_at(path: &Path) {
    match log::to_file(path) {
        Ok(()) => log::event(
            "desktop.log_file",
            &[
                ("path", &path.to_string_lossy()),
                ("build", &crate::build_info::id()),
            ],
        ),
        Err(error) => log::failure("desktop.log_file_unavailable", &error),
    }
}

/// Tells the person `text` through `notice`, and keeps it in the log: advice such as which
/// driver to install is the line a person without a terminal needs most. `text` is our own
/// wording, so it may be logged whole.
pub(crate) fn announce(notice: crate::app::Notice, text: &str) {
    log::event("desktop.notice", &[("text", text)]);
    notice(text);
}

/// Stops the client with `text` for the person: on the terminal and, on Windows, where a release
/// client has no console, in a message box. `text` may carry the error's own words; the log gets
/// only the fixed `kind`, since an error's text can carry a path.
pub(crate) fn announce_fatal(notice: crate::app::Notice, kind: &str, text: &str) {
    log_fatal(kind);
    notice(text);
    dialog::error("Soyeht", text);
}

fn log_fatal(kind: &str) {
    log::event("desktop.fatal", &[("kind", kind)]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn log_lines_reach_the_file_in_the_log_directory() {
        let root = std::env::temp_dir().join(format!("soyeht-desktop-log-{}", std::process::id()));
        let path = root.join("logs").join("desktop.log");
        start_at(&path);
        announce(|_| (), "soyeht: install a driver");
        log_fatal("window_open_failed");
        log::event("desktop.test_event", &[]);

        let text = fs::read_to_string(&path).unwrap();
        let located = text
            .lines()
            .find(|line| line.contains("desktop.log_file"))
            .unwrap();
        assert!(located.starts_with(r#"{"event":"desktop.log_file","path":""#));
        assert!(located.contains("desktop.log"));
        assert!(text.contains(r#"{"event":"desktop.notice","text":"soyeht: install a driver"}"#));
        assert!(text.contains(r#"{"event":"desktop.fatal","kind":"window_open_failed"}"#));
        assert!(text.contains(r#"{"event":"desktop.test_event"}"#));
        // The open log may keep the directory on Windows; a leftover in the temp directory is
        // not a failure of what this tests.
        fs::remove_dir_all(root).ok();
    }
}
