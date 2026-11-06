//! What a terminal pane's history costs in memory. The window keeps one screen for every
//! terminal pane, each with `TERMINAL_SCROLLBACK_LINES` lines of history. Unmodified vt100 0.16.2
//! stores every cell of every row of it (32 bytes, whatever the row holds); the fork this client
//! builds with (ADR 0144) holds a row that scrolled out packed, with no blank end of the line,
//! and unpacks only the rows in view. Each screen is kept until the end,
//! so a later one cannot reuse the memory of an earlier one. This prints the resident memory a
//! screen takes once its history is full, at several widths, with plain and with coloured output.
//!
//! Run it with the release profile for numbers that match the shipped window, from `apps/desktop`:
//! `cargo run --release -p soyeht-client --example scrollback_memory`

use soyeht_client::screen::TerminalScreen;
use std::process::Command;

const ROWS: u16 = 40;
const LINES: usize = 200_000;
const CHUNK_BYTES: usize = 64 * 1024;

fn resident_mb() -> f64 {
    let pid = std::process::id().to_string();
    Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .map_or(0.0, |output| {
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<f64>()
                .map_or(0.0, |kilobytes| kilobytes / 1024.0)
        })
}

/// A screen of `columns` with its history full of `line`, and the memory it took. The screen is
/// returned so that it is still held when the next one is measured.
fn full_screen(columns: u16, line: impl Fn(usize) -> String) -> (TerminalScreen, f64) {
    let before = resident_mb();
    let mut screen = TerminalScreen::new(ROWS, columns);
    let mut chunk = String::new();
    for number in 0..LINES {
        chunk.push_str(&line(number));
        chunk.push_str("\r\n");
        if chunk.len() >= CHUNK_BYTES {
            screen.feed(chunk.as_bytes());
            chunk.clear();
        }
    }
    screen.feed(chunk.as_bytes());
    (screen, resident_mb() - before)
}

fn main() {
    let mut held = Vec::new();
    for columns in [80, 120, 200] {
        let (plain, plain_mb) = full_screen(columns, |n| {
            format!("{n} some typical log line of about sixty characters, ok")
        });
        let (coloured, coloured_mb) = full_screen(columns, |n| {
            format!("\x1b[32m{n}\x1b[0m some \x1b[1;31mtypical\x1b[0m log line of sixty characters")
        });
        eprintln!(
            "{columns} columns, history full: plain {plain_mb:.1} MB, coloured {coloured_mb:.1} MB"
        );
        held.push((plain, coloured));
    }
}
