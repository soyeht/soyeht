// A release build on Windows is a window program: it opens no console. Its log lines
// (`soyeht_platform::log`) go to standard error, which a shell can redirect (`2> soyeht.log`), and
// always to `logs/desktop.log` in the profile directory (`logging`).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    // The advice to the person (a missing driver, a missing socket) is text for a terminal: this is
    // the one place that prints it. `logging::announce` also logs it, and on a failure before the
    // window exists shows it in a message box.
    soyeht_desktop::run(|text| eprintln!("{text}"));
}
