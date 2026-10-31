//! The panic hook: what a panic leaves in a report.

use super::{
    frames,
    report::{LINE_CHARS, MAX_FRAMES, MAX_LOG_LINES, MAX_SYMBOLS, Program, Report, one_line},
    store,
};
use crate::{diagnostics::Redactor, log, process::detached::logs_directory};
use std::{
    backtrace::Backtrace,
    cell::Cell,
    panic::{self, PanicHookInfo},
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

/// A program writes at most this many reports, so a panic in a loop cannot push the older
/// reports out of the folder.
const MAX_PER_PROCESS: usize = 3;
/// The name of the thread a program ends with when it panics.
const MAIN_THREAD: &str = "main";
/// A location is cut here.
const LOCATION_CHARS: usize = 160;

/// What a program tells the hook about itself.
#[derive(Clone, Debug)]
pub struct Settings {
    pub program: Program,
    /// The profile's data directory, where `crashes/` and `logs/` are.
    pub data: PathBuf,
    /// The program's own log file in `logs/`, whose last lines the report keeps.
    pub log: &'static str,
}

struct State {
    settings: Settings,
    redactor: Redactor,
    written: AtomicUsize,
}

static STATE: OnceLock<State> = OnceLock::new();

thread_local! {
    /// A panic inside the hook does not start another report.
    static WRITING: Cell<bool> = const { Cell::new(false) };
}

/// Writes a report to `crashes/` of the data directory each time a thread of this program
/// panics, then runs the hook that was there. The program keeps its behaviour: a panic unwinds,
/// a main thread that panics ends the program, and a supervisor of the system restarts it.
/// The second call changes nothing.
pub fn install(settings: Settings) {
    let state = State {
        settings,
        redactor: Redactor::current(),
        written: AtomicUsize::new(0),
    };
    if STATE.set(state).is_err() {
        return;
    }
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        if let Some(state) = STATE.get() {
            state.on_panic(info);
        }
        previous(info);
    }));
}

impl State {
    fn on_panic(&self, info: &PanicHookInfo<'_>) {
        if WRITING.replace(true) {
            return;
        }
        if self.written.fetch_add(1, Ordering::Relaxed) < MAX_PER_PROCESS {
            self.write(&self.report(info));
        }
        WRITING.set(false);
    }

    fn write(&self, report: &Report) {
        match store::write(&self.settings.data, report) {
            Ok(()) => log::event(
                "crash.report_written",
                &[
                    ("program", report.program.name()),
                    ("kind", &report.kind),
                    ("fatal", if report.fatal { "yes" } else { "no" }),
                ],
            ),
            Err(error) => log::failure("crash.report_failed", &error),
        }
    }

    fn report(&self, info: &PanicHookInfo<'_>) -> Report {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed");
        let strip = |text: String| self.redactor.strip_home(&text);
        Report {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            system: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            program: self.settings.program,
            time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs()),
            pid: std::process::id(),
            thread: one_line(name, LINE_CHARS),
            fatal: name == MAIN_THREAD,
            kind: kind(info).to_owned(),
            location: info
                .location()
                .map(|place| {
                    strip(one_line(
                        &format!(
                            "{}:{}:{}",
                            source_path(place.file()),
                            place.line(),
                            place.column()
                        ),
                        LOCATION_CHARS,
                    ))
                })
                .unwrap_or_default(),
            frames: frames::capture(MAX_FRAMES),
            symbols: symbols().into_iter().map(strip).collect(),
            log: self.log_tail(),
        }
    }

    /// The last structured lines of the program's own log, with paths, names and tokens removed.
    /// A line that is not a structured one (what the default hook prints, with the panic's
    /// message) is left out.
    fn log_tail(&self) -> Vec<String> {
        let path = logs_directory(&self.settings.data).join(self.settings.log);
        let Ok(lines) = crate::diagnostics::tail(&path, MAX_LOG_LINES * 4) else {
            return Vec::new();
        };
        let mut kept: Vec<String> = lines
            .iter()
            .filter(|line| line.starts_with("{\"event\":"))
            .map(|line| one_line(&self.redactor.redact(line), LINE_CHARS))
            .collect();
        kept.drain(..kept.len().saturating_sub(MAX_LOG_LINES));
        kept
    }
}

/// A label for the panic, from the shapes the standard library gives its own. The message is
/// never kept: it can carry what the person was working on.
fn kind(info: &PanicHookInfo<'_>) -> &'static str {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or_default();
    message_kind(message)
}

pub(super) fn message_kind(message: &str) -> &'static str {
    const KINDS: [(&str, &str); 9] = [
        ("called `Option::unwrap()` on a `None`", "unwrap_none"),
        ("called `Result::unwrap()` on an `Err`", "unwrap_err"),
        ("index out of bounds", "index_out_of_bounds"),
        ("attempt to ", "arithmetic"),
        ("already borrowed", "borrow"),
        ("already mutably borrowed", "borrow"),
        ("slice index", "slice_range"),
        ("range end index", "slice_range"),
        ("range start index", "slice_range"),
    ];
    if let Some((_, kind)) = KINDS.iter().find(|(start, _)| message.starts_with(start)) {
        kind
    } else if message.contains("is not a char boundary") {
        "char_boundary"
    } else if message.contains("PoisonError") || message.contains("poisoned") {
        "poisoned_lock"
    } else {
        "explicit"
    }
}

/// The path of a source file as the compiler recorded it, without any folder of the person: a
/// path inside the workspace is relative already, a dependency's is cut at its crate folder
/// (`tokio-1.40.0/src/x.rs`), the standard library's drops the compiler's commit, and any
/// other absolute path keeps its last two parts.
pub(super) fn source_path(file: &str) -> String {
    let path = file.replace('\\', "/");
    if let Some((_, rest)) = path.split_once("/registry/src/") {
        return rest
            .split_once('/')
            .map_or(rest, |(_, crate_path)| crate_path)
            .to_owned();
    }
    if let Some(rest) = path.strip_prefix("/rustc/") {
        return rest
            .split_once('/')
            .map_or(rest, |(_, library)| library)
            .to_owned();
    }
    let absolute = path.starts_with('/') || path.get(1..3) == Some(":/");
    if absolute {
        let parts: Vec<&str> = path.rsplit('/').take(2).collect();
        return format!(
            ".../{}/{}",
            parts.get(1).unwrap_or(&""),
            parts.first().unwrap_or(&"")
        );
    }
    path.split('/')
        .filter(|part| *part != ".." && *part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

/// The frames' names, when the program carries symbols, as `name (file.rs:12)`. Frames of the
/// panic machinery itself are left out.
fn symbols() -> Vec<String> {
    symbol_names(&Backtrace::force_capture().to_string())
}

pub(super) fn symbol_names(backtrace: &str) -> Vec<String> {
    const MACHINERY: [&str; 6] = [
        "std::backtrace",
        "soyeht_platform::crash",
        "std::panicking",
        "core::panicking",
        "rust_begin_unwind",
        "__rust_end_short_backtrace",
    ];
    let mut names: Vec<String> = Vec::new();
    let mut skipping = false;
    for line in backtrace.lines() {
        let line = line.trim();
        if let Some(place) = line.strip_prefix("at ") {
            if !skipping && let Some(last) = names.last_mut() {
                last.push_str(&format!(" ({})", file_line(place)));
            }
            continue;
        }
        let Some((index, name)) = line.split_once(": ") else {
            continue;
        };
        skipping = !index.chars().all(|c| c.is_ascii_digit())
            || name.contains("__mh_execute_header")
            || name == "<unknown>"
            || (names.is_empty() && MACHINERY.iter().any(|part| name.contains(part)))
            || name.contains('/');
        if !skipping {
            names.push(one_line(name, LINE_CHARS));
        }
    }
    names.truncate(MAX_SYMBOLS);
    names
}

/// `file.rs:12` of `/some/folder/file.rs:12:5`.
fn file_line(place: &str) -> String {
    let normalized = place.replace('\\', "/");
    let name = normalized.rsplit('/').next().unwrap_or_default();
    name.splitn(3, ':').take(2).collect::<Vec<_>>().join(":")
}
