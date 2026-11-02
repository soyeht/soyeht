//! The panic hook: what a panic leaves in a report.
//!
//! The hook runs inside a panic. A panic raised inside a panic hook aborts the process before any
//! code of ours can see it, so nothing here can be guarded against it: the hook only does what
//! cannot panic (no indexing, no arithmetic that can overflow, no `unwrap`) and returns every I/O
//! failure as a value. Its own log line only tries the log file's lock. The library calls it
//! makes (publishing the file) may write an ordinary log line, which waits for that lock; the lock
//! is held only while a line is written to the file, by code that does not panic, so a thread
//! cannot be inside it when it panics.

use super::{
    frames,
    report::{LINE_CHARS, MAX_FRAMES, MAX_LOG_LINES, Program, Report, one_line, unix_seconds},
    store,
};
use crate::{diagnostics::Redactor, log, process::detached::logs_directory};
use std::{
    cell::Cell,
    panic::{self, AssertUnwindSafe, PanicHookInfo, catch_unwind},
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

/// A program writes at most this many reports of panics that do not end it (a task, a worker
/// thread, a panic it caught), so a panic in a loop cannot push the older reports out. A panic that
/// ends the program is always written: there is at most one.
const MAX_NON_FATAL: usize = 3;
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
    non_fatal: AtomicUsize,
}

static STATE: OnceLock<State> = OnceLock::new();

thread_local! {
    /// How many [`caught`] calls this thread is inside.
    static CAUGHT: Cell<usize> = const { Cell::new(0) };
}

/// Runs `work`, and gives a panic in it back as an `Err` instead of letting it go on. For code that
/// catches panics on purpose and goes on (a parser fed by a program in a pane, work an engine
/// task does): the hook reports such a panic as one that did not end the program, even on the
/// thread named `main`, and counts it against the cap of three. A panic caught any other way
/// on the main thread is taken for the end of the program.
pub fn caught<R>(work: impl FnOnce() -> R) -> std::thread::Result<R> {
    CAUGHT.with(|inside| inside.set(inside.get().saturating_add(1)));
    let result = catch_unwind(AssertUnwindSafe(work));
    CAUGHT.with(|inside| inside.set(inside.get().saturating_sub(1)));
    result
}

fn inside_caught() -> bool {
    // A thread that is being torn down has no cell; it is not inside a caught call.
    CAUGHT.try_with(|inside| inside.get() > 0).unwrap_or(false)
}

/// Writes a report to `crashes/` of the data directory each time a thread of this program
/// panics, then runs the hook that was there. The program keeps its behaviour: a panic unwinds,
/// a main thread that panics ends the program, and a supervisor of the system restarts it.
/// The second call changes nothing.
pub fn install(settings: Settings) {
    let state = State {
        settings,
        redactor: Redactor::current(),
        non_fatal: AtomicUsize::new(0),
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
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed");
        let fatal = name == MAIN_THREAD && !inside_caught();
        if fatal || self.non_fatal.fetch_add(1, Ordering::Relaxed) < MAX_NON_FATAL {
            self.write(&self.report(info, name, fatal));
        }
    }

    fn write(&self, report: &Report) {
        match store::write(&self.settings.data, report) {
            Ok(()) => log::try_event(
                "crash.report_written",
                &[
                    ("program", report.program.name()),
                    ("kind", &report.kind),
                    ("fatal", if report.fatal { "yes" } else { "no" }),
                ],
            ),
            Err(error) => log::try_failure("crash.report_failed", &error),
        }
    }

    fn report(&self, info: &PanicHookInfo<'_>, thread: &str, fatal: bool) -> Report {
        let strip = |text: String| self.redactor.strip_home(&text);
        Report {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            system: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            program: self.settings.program,
            time: unix_seconds(),
            pid: std::process::id(),
            thread: one_line(thread, LINE_CHARS),
            fatal,
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
            log: self.log_tail(),
        }
    }

    /// The last structured lines of the program's own log, with paths, names and tokens removed.
    /// `tail` keeps only structured lines, so what the default hook prints (with the panic's
    /// message) is not among them.
    fn log_tail(&self) -> Vec<String> {
        let path = logs_directory(&self.settings.data).join(self.settings.log);
        crate::diagnostics::tail(&path, MAX_LOG_LINES)
            .map(|lines| {
                lines
                    .iter()
                    .map(|line| one_line(&self.redactor.redact(line), LINE_CHARS))
                    .collect()
            })
            .unwrap_or_default()
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
