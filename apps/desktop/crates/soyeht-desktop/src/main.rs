// A release build on Windows is a window program: it opens no console. Its log lines
// (`soyeht_platform::log`) go to standard error, so they appear when a shell started it with
// `2> soyeht.log`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    // The advice to the person (a missing driver, a missing socket) is text for a terminal, not a
    // log line: this is the one place that prints it.
    soyeht_desktop::run(|text| eprintln!("{text}"));
}
