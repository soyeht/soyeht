# Contract 16 · Crash reports

Status: v1. Decision: ADR 0127.
Code: `soyeht_platform::crash` (the hook, the report and its store), used through `soyeht
crash-reports` and `soyeht diagnostics`. Texts: `crashNotice*` of `x-clientPolicy.texts` in
`contracts/protocol.json`.

When a Soyeht program crashes, it leaves a small report on the person's own computer. Nothing is
uploaded and nothing is sent unless the person chooses to: the next window says that it closed
unexpectedly, and "Copy report" puts the report on the clipboard to paste into an issue.

## 1. When a report is written

| Program | `program` | Source | Where it is installed |
|---|---|---|---|
| The engine | `engine` | a panic | `soyeht_engine::launch::run`, so the product engine and the test engine |
| The supervisor | `supervisor` | a panic | `soyeht-ptyd`'s `main` |
| The Windows and Linux app | `desktop` | a panic | `soyeht-desktop`'s `main` |
| The Mac app | `mac` | the system's crash diagnostic (MetricKit) | the app forwards it to `soyeht crash-reports import` (section 5) |

A Rust program installs one panic hook, `soyeht_platform::crash::install`, once its data directory
is known. The hook writes the report, then runs the hook that was there (the default one prints the
panic to standard error, as before). It changes nothing else: the panic unwinds as it always did, a
panic on the main thread ends the program, and the service manager that starts the engine and the
supervisor at login starts them again (ADR 0114). `panic = "unwind"` stays (`Cargo.toml`).

The hook runs inside a panic, where a second panic aborts the process before anything can catch it.
So it does only what cannot panic (no indexing, no arithmetic that can overflow, no `unwrap`; an I/O
failure is a value). Its own log line only tries the log file's lock: it goes to standard error and,
when the lock is free, to the file. The library call that publishes the file may write an ordinary
log line, which waits for that lock; the lock is held only while a line is written, by code that does
not panic, so a thread is never inside it when it panics.

`fatal` says whether the program ended: the panic was not caught, on the thread named `main`, or the
system said the app crashed. A panic that only ends a task or a worker thread, and a panic that
code catches on purpose and goes on from, are written (`fatal: no`) and appear in the diagnostics, but
they are not "closed unexpectedly" and do not raise the notice. Code that catches panics runs inside
`soyeht_platform::crash::caught`, which tells the hook so even on the main thread (the Windows and
Linux window's terminal parser, which the GPUI main thread drives, and the engine's background work).
A program writes at most 3 reports of such panics; an uncaught panic on the main thread is always
written, and since it ends the program there is one, so a loop of caught panics cannot hide it.

Not covered, because a panic hook never runs for them: a stack overflow, an out-of-memory abort,
`SIGKILL`, a fatal signal in native code, a power loss. The Mac app gets the signals from the
system. The command line (`soyeht`) and the MCP server (`soyeht-mcp`) install no hook; they print
their error and exit.

## 2. The report

A report is a text file of the profile's data directory, `crashes/crash-<time>-<program>-<pid>-<n>.txt`
(`<time>` is 12 digits of seconds since the epoch, so the names sort by time; `<n>` tells apart the
reports one program writes in the same second). The folder and the files are owner-only.

```
soyeht-crash-report 1
version: 0.1.0
system: macos aarch64
program: engine
time: 1761907872
pid: 4242
thread: main
fatal: yes
kind: unwrap_none
location: soyeht-engine/src/launch.rs:57:21
frames:
soyeht-engine+0x4f80c
libsystem_pthread.dylib+0x6c58
log:
{"event":"engine.ready"}
```

| Line | Content |
|---|---|
| first | `soyeht-crash-report 1`. Another first line is not a report of this format and is ignored |
| `version` | the product version of the program (`contracts/release.json`, read as `CARGO_PKG_VERSION`) |
| `system` | the operating system and architecture |
| `program` | `engine`, `supervisor`, `desktop` or `mac` |
| `time`, `pid` | when, and which process |
| `thread` | the thread's name (`main`, `tokio-runtime-worker`, `unnamed`) |
| `fatal` | `yes` or `no`, section 1 |
| `kind` | a label, never the panic's message: `unwrap_none`, `unwrap_err`, `index_out_of_bounds`, `arithmetic`, `borrow`, `slice_range`, `char_boundary`, `poisoned_lock`, `explicit` (the program panicked with a message of its own); `signal_<NAME>` or `exception_<n>` for the Mac app |
| `location` | the source file, line and column of the panic. A path inside the workspace is relative; a dependency's starts at its crate folder (`tokio-1.40.0/src/x.rs`); the standard library's has no compiler commit; any other absolute path keeps its last two parts. No folder of the person |
| `frames:` | one line per frame of the stack, innermost first: `module+0xoffset`, the module's file name and the frame's offset from its load address |
| `log:` | the last structured lines (`{"event":...}`) of the program's own log (`engine.log`, `ptyd.log`, `desktop.log`), after the redaction of contract 14 |

The message of a panic is never kept: it can carry what the person was working on (a file name, a
fragment of text). For the same reason the log lines kept are only the structured ones, which hold
event names and failure kinds (ADR 0088). What the default panic hook prints to standard error, which
the services' output redirection and the engine's copy of it write into the same log file, has no such
shape and is not kept: neither in a report nor in `soyeht diagnostics`, whose log sections keep only
structured lines for the same reason (contract 14).

**Symbolicating.** A frame is an offset, not a name, because a shipped binary has no symbols. With
the unstripped build of that exact `version` the offset is the address to look up (`atos -o BINARY
-l 0x0 ADDRESS`, `addr2line -e BINARY`). The offset is the return address, so look up the one
before it. The release flow must keep the unstripped program of each version (`docs/release.md`; not yet done).

### Limits

All limits are constants of `soyeht_platform::crash`; they bound a report whoever wrote it.

| Limit | Value |
|---|---|
| reports kept | 8; the oldest is removed when a new one is written |
| reports one program writes | 3 per process for panics that do not end it (tasks, workers, caught); the one that does is always written |
| report size | 16 KiB; log lines are dropped first, then frames |
| frames, log lines | 48, 20 |
| a line | 240 characters; a header value 160 |

A text that does not parse, is larger than twice the report size, or has no known program is not a
report and is left out. A file that is a link is not followed.

## 3. The notice

The window or app asks `soyeht crash-reports status` when it starts. If reports are pending it
shows one quiet notice, not a dialog and not a blocker:

- `crashNoticeTitle`: "Soyeht closed unexpectedly"
- `crashNoticeSummary`: "A crash report was saved on this computer. Nothing was sent."
- `crashNoticeCopy`: "Copy report"
- `crashNoticeDismiss`: "Dismiss"

A report is **pending** when it is `fatal: yes` and its file name is not listed in
`crashes/acknowledged` (names of the reports seen, one per line; names of reports that are gone are
dropped), so a clock that steps back, or two programs that crash in the same second, cannot hide one. **Copy report** is the same flow as "Copy diagnostics" (contract 14,
section 4, with its `diagnostics*` messages) and then acknowledges; **Dismiss** acknowledges
(`soyeht crash-reports acknowledge NAME...`), naming the reports the notice showed: one that arrived
while the notice was up is still pending. An acknowledged report stays in the folder, so the diagnostics
still carry it until the bound removes it, and the notice does not come back for it.

`status` prints one JSON object: `pending` (a count), `reports` (their file names) and `latest`
(`program` and `time` of the newest pending report, or `null`). `acknowledge` prints `acknowledged`
(how many it marked); with no name it marks every pending report.
Both work with no engine, since the engine is what may have crashed.

## 4. The report joins Copy diagnostics

`soyeht diagnostics` (contract 14) ends with a `### Crash reports` section: each report of the
folder, oldest first, under `#### <program> · <time> · closed` in a fenced block. The location,
and frames pass through the home-folder removal and the log lines through the whole
redaction once more, since a file in the folder may not be ours. With none, the section says
`(none)`. So "Copy diagnostics" and "Copy report" copy the same text, and a client adds no rule.

## 5. The Mac app

The Mac app registers a MetricKit subscriber. For each crash diagnostic the system delivers, it
writes `MXCrashDiagnostic.jsonRepresentation()` to a file in `crashes/` and runs the `soyeht` of
its bundle (contract 14, section 4): `soyeht crash-reports import FILE`. The program refuses a file that is not an `incoming-*` file directly in `crashes/` or is a link (it
deletes what it imports, so it is not pointed at anything else), and one that has neither the
diagnostic's `diagnosticMetaData` nor its `callStackTree`. It reads the diagnostic, keeps only the facts of section 2 (the signal or the exception type, the version and
architecture, the module and offset of each frame of the crashed thread's stack), writes a report
of the program `mac` and deletes the file. The termination reason and every other free text of the
diagnostic are dropped. A file larger than 1 MiB, or that is not a diagnostic, is refused and left in
place; the app deletes it, and sweeps any `incoming-*` file left from an earlier run when it starts.

The system delivers a crash diagnostic on a later launch, so the notice for a Mac crash can come a
launch after the crash.

## 6. Not covered

- The iPhone and iPad app: it has no `soyeht` program to run and no "Copy diagnostics" yet. Its
  MetricKit subscriber and a Swift report writer are a follow-up.
- A crash of the engine while the window stays open raises the notice at the next start of the
  window, not at the moment of the crash.
- Sending a report anywhere. If that is ever added it needs the person's consent for each report
  and its own ADR.
