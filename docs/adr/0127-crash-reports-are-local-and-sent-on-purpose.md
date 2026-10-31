# ADR 0127: Crash reports are written locally and sent on purpose

- Status: accepted
- Date: 2026-10-31

## Context

Before the public launch a crash left nothing behind but a line of the default panic hook in a
log, and nothing at all on the Mac app. A person whose engine, supervisor or app had crashed could
not tell us what happened, and we could not tell them that it had. Copy diagnostics (ADR 0118)
gives them a redacted report to paste, but it does not know about a crash. The product has no
account and no server of its own, and a crash is when a person is least likely to want something
sent without asking.

## Decision

**A crash writes a small report on the computer and nothing leaves it.** The shape, the limits and
the notice are contract 16.

- **One hook for every Rust program**, `soyeht_platform::crash::install`, in the layer that
  already holds the log, the redaction and the owner-only files. The engine installs it in
  `launch::run` (both the product and the test program run it), the supervisor in its `main`, the
  Windows and Linux app in its `main`. It writes, then calls the hook that was there; it never
  aborts, never exits, and `panic = "unwind"` stays, so the programs crash and restart as they did
  (ADR 0114's service manager restarts the engine and the supervisor).
- **The message of the panic is never recorded**, only a label from the shapes the standard
  library gives its own (`unwrap_none`, `index_out_of_bounds`, ...) and the source location. A
  message can carry a file name or a fragment of what the person was doing, and ADR 0088 keeps
  message text out of the logs for the same reason. The log lines in the report are the program's
  own structured lines, redacted as in contract 14.
- **The backtrace is `module+0xoffset`, not names.** A shipped program is stripped, and stripping
  is a decision (the engine hashes its own executable at start, and the profile says so in `Cargo.toml`). The
  hook captures the return addresses with the system's own call (`backtrace` and `dladdr` on Unix,
  `RtlCaptureStackBackTrace` on Windows) and records the module's file name and the offset from
  its load address, which survives ASLR. With the unstripped build of that version, the offset is a
  name. A program that does carry symbols (a development build) adds them in `symbols`. The
  alternative, keeping symbols in the release, costs binary size and start time for every person to
  save a lookup for the few who crash. No third-party backtrace crate is added.
- **A report is a text file, not JSON.** `soyeht-platform` has no parser and no Soyeht
  dependency; the text form is read and written there, pasted as it is into the diagnostics, and
  small enough to read by eye. The first line carries the format number.
- **Bounded**: 8 reports, 3 per process, 16 KiB each, the oldest removed. A panic in a loop cannot
  fill the disk or push out the report that mattered.
- **"Closed unexpectedly" means the program ended.** A panic on the thread named `main` ends the
  program; one in a task or a worker is written with `fatal: no` and only reaches the diagnostics.
  The notice is for the first kind.
- **Clients run the program, as for the diagnostics (ADR 0118 amendment).** `soyeht crash-reports
  status | acknowledge | import FILE` is the one seam for the Windows and Linux app and the Mac
  app. "Copy report" is "Copy diagnostics" (the report is part of it), so no client holds a
  redaction rule and no second copy path exists. The notice's four sentences are
  `x-clientPolicy.texts`, read through the generated forms.
- **The Mac app forwards MetricKit to the program** instead of parsing it in Swift. The system
  gives a crash diagnostic as JSON on a later launch; the app writes it to a file and the program
  keeps the signal or exception type, the version, the architecture and the module and offset of
  each frame, and drops the termination reason and every free text. The parsing, the bound and the
  format stay in Rust, so there is one writer of reports.

### The make-it-crash hook

The engine test program (`tests/test-engine/program.rs`, built only with `test-hooks`, ADR 0091)
panics in the closure `launch::run` calls with the data directory when a marker file
`test-crash-at-start` is in it. That is after the report hook is installed and on the main thread,
so the integration test sees a real fatal crash of a real engine process with no branch in
production code. The supervisor and the apps are covered by the platform test of the hook (a
process of its own that installs it and panics on threads) and by their `main` calling `install`.

## Consequences

- A person who sends "Copy diagnostics" after a crash sends the crash with it; a maintainer who
  gets a report and has the unstripped build of that version can name every frame.
- The release flow must keep the unstripped program of every version it ships.
- A stack overflow, an out-of-memory abort and a kill leave no report from a Rust program: the
  hook cannot run there. The Mac app gets signals from MetricKit; a Rust program killed by a
  signal is a follow-up (a signal handler is async-signal-unsafe territory and would be its own
  ADR).
- The iPhone and iPad app, which cannot run the program, has no crash notice yet.
- The engine's crash while a window stays open raises the notice at the window's next start.

## Not verified

The Windows frame capture (`RtlCaptureStackBackTrace`, `GetModuleHandleExW`) compiles and lints
for `x86_64-pc-windows-msvc`; it did not run on Windows. The Linux capture compiles for
`x86_64-unknown-linux-gnu` and was not run on Linux. The MetricKit import is tested with a diagnostic
written from the documented JSON shape, not with one the system produced.
