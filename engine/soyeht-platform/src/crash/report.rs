//! The crash report and its text form (contract 16).
//!
//! A report is a small text file, not JSON, so this crate (which has no parser) reads and writes
//! it, and `soyeht diagnostics` can paste it as it is. Every value is one bounded line.

use std::fmt;

/// The first line of every report; the number is the format.
pub const MAGIC: &str = "soyeht-crash-report 1";

/// Most frames of a backtrace a report keeps.
pub const MAX_FRAMES: usize = 48;
/// Most log lines a report keeps.
pub const MAX_LOG_LINES: usize = 20;
/// A frame, a symbol or a log line is cut here.
pub const LINE_CHARS: usize = 240;
/// A header value is cut here.
const VALUE_CHARS: usize = 160;
/// A whole report is at most this long; what does not fit is left out, log lines first.
pub const MAX_REPORT_BYTES: usize = 16 * 1024;

/// Which Soyeht program crashed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Program {
    Engine,
    Supervisor,
    Desktop,
    Mac,
}

impl Program {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Engine => "engine",
            Self::Supervisor => "supervisor",
            Self::Desktop => "desktop",
            Self::Mac => "mac",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [Self::Engine, Self::Supervisor, Self::Desktop, Self::Mac]
            .into_iter()
            .find(|program| program.name() == name)
    }
}

impl fmt::Display for Program {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// What a crash left behind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub version: String,
    /// The operating system and architecture.
    pub system: String,
    pub program: Program,
    /// Seconds since the Unix epoch.
    pub time: u64,
    pub pid: u32,
    pub thread: String,
    /// Whether the program ended: the panic was not caught, on its main thread, or the system
    /// said it crashed. A panic that only ended a task, or that code caught, is not fatal.
    pub fatal: bool,
    /// A bounded label for what went wrong, never the message text.
    pub kind: String,
    /// The source file, line and column the panic came from, with no folder of the person.
    pub location: String,
    /// `module+0xoffset` of each frame, innermost first, for a symbolicator.
    pub frames: Vec<String>,
    /// The last structured log lines the program wrote.
    pub log: Vec<String>,
}

/// One value on one line: no control character, no more than `limit` characters.
pub(super) fn one_line(text: &str, limit: usize) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(limit)
        .collect::<String>()
        .trim()
        .to_owned()
}

impl Report {
    /// The stored form. Frames and log lines are dropped, the log lines first,
    /// until it is no longer than [`MAX_REPORT_BYTES`].
    pub fn to_text(&self) -> String {
        let mut report = self.clone();
        loop {
            let text = report.render();
            if text.len() <= MAX_REPORT_BYTES {
                return text;
            }
            if report.log.pop().is_none() && report.frames.pop().is_none() {
                return text.chars().take(MAX_REPORT_BYTES).collect();
            }
        }
    }

    fn render(&self) -> String {
        let mut lines = vec![
            MAGIC.to_owned(),
            format!("version: {}", one_line(&self.version, VALUE_CHARS)),
            format!("system: {}", one_line(&self.system, VALUE_CHARS)),
            format!("program: {}", self.program),
            format!("time: {}", self.time),
            format!("pid: {}", self.pid),
            format!("thread: {}", one_line(&self.thread, VALUE_CHARS)),
            format!("fatal: {}", if self.fatal { "yes" } else { "no" }),
            format!("kind: {}", one_line(&self.kind, VALUE_CHARS)),
            format!("location: {}", one_line(&self.location, VALUE_CHARS)),
        ];
        for (name, section, limit) in [
            ("frames:", &self.frames, MAX_FRAMES),
            ("log:", &self.log, MAX_LOG_LINES),
        ] {
            lines.push(name.to_owned());
            lines.extend(
                section
                    .iter()
                    .take(limit)
                    .map(|line| one_line(line, LINE_CHARS)),
            );
        }
        lines.push(String::new());
        lines.join("\n")
    }

    /// Reads the stored form. A text that is not a report of this format is `None`, so a file
    /// that was altered, truncated or put there by someone else is never trusted.
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() > MAX_REPORT_BYTES * 2 {
            return None;
        }
        let mut lines = text.lines();
        if lines.next()? != MAGIC {
            return None;
        }
        let mut report = Self {
            version: String::new(),
            system: String::new(),
            program: Program::Engine,
            time: 0,
            pid: 0,
            thread: String::new(),
            fatal: false,
            kind: String::new(),
            location: String::new(),
            frames: Vec::new(),
            log: Vec::new(),
        };
        let mut program = None;
        let mut section = None;
        for line in lines {
            match line {
                "frames:" | "log:" => section = Some(line),
                _ => match section {
                    None => {
                        let Some((key, value)) = line.split_once(": ") else {
                            continue;
                        };
                        let value = one_line(value, VALUE_CHARS);
                        match key {
                            "version" => report.version = value,
                            "system" => report.system = value,
                            "program" => program = Program::parse(&value),
                            "time" => report.time = value.parse().ok()?,
                            "pid" => report.pid = value.parse().ok()?,
                            "thread" => report.thread = value,
                            "fatal" => report.fatal = value == "yes",
                            "kind" => report.kind = value,
                            "location" => report.location = value,
                            _ => {}
                        }
                    }
                    Some("frames:") if report.frames.len() < MAX_FRAMES => {
                        report.frames.push(one_line(line, LINE_CHARS));
                    }
                    Some("log:") if report.log.len() < MAX_LOG_LINES => {
                        report.log.push(one_line(line, LINE_CHARS));
                    }
                    Some(_) => {}
                },
            }
        }
        report.program = program?;
        Some(report)
    }
}

/// Seconds since the Unix epoch, 0 when the clock is before it.
pub fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// `seconds` since the Unix epoch as `2026-10-31 10:11:12 UTC`.
pub fn utc(seconds: u64) -> String {
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let rest = seconds % 86_400;
    // Days since 1970-01-01 to a civil date (the proleptic Gregorian calendar).
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}
