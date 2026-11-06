# ADR 0144: The Linux and Windows window and the Android core use a vt100 fork with a compact scrollback

- Status: accepted
- Date: 2026-11-05

## Context

The window for Linux and Windows (ADR 0084) and the Android core (ADR 0134) turn the engine's terminal bytes
into styled rows with the `vt100` crate. Each terminal pane keeps a parsed screen, for every pane of every
workspace, with `scrollbackLines` of history: 10,000 lines (`contracts/protocol.json`, `x-clientPolicy`).

`vt100` 0.16.2 stores every row it scrolls out of the screen as a full-width row of 32-byte cells, whatever the
line holds, in a private queue that nothing can shrink, trim or read before the row is dropped. Measured with
a real `TerminalScreen` and 200,000 lines of ordinary output (`apps/desktop/crates/soyeht-client/examples/
scrollback_memory.rs`), one pane with its history full takes:

| width | `vt100` 0.16.2 |
| --- | --- |
| 80 columns | 27 MB |
| 120 columns | 40 MB |
| 200 columns | 71 MB |

Colour makes no difference. Ten such panes at 200 columns are 700 MB, on a phone as well as on a desktop. The
history is the same 10,000 lines the person scrolls through, so the way to a smaller screen is how the rows are
held, not how many.

Ways out that were looked at:

- **Fewer lines.** It changes what the person can scroll back to. `scrollbackLines` stays at 10,000
  (the Mac and iPhone move to it too).
- **Drop the screens of panes that are not shown and rebuild them from the engine's output.** The engine keeps
  the last 2 MiB of a pane in memory (`TAIL_BYTES`) and, when the pane records, up to 32 MiB on disk; a pane that
  does not record keeps only the 2 MiB. Ten thousand lines of coloured output can be more than that. A replay
  that starts in the middle of the output also does not give back the screen the terminal had (alternate
  screen, attributes, modes), which is what a reconnect already accepts and a workspace switch must not.
- **Pack a hidden pane's screen and history and rebuild it when it is shown.** `reflow.rs` documents what a
  rebuilt screen loses (autowrap off, insert mode, a charset, the scroll region, the saved cursor), and the
  rebuild is a pause when the person switches workspace.
- **Wrap the parser.** `vt100` gives no signal when a row scrolls out, and `scrollback_len()` returns only the
  cap, so rows cannot be taken out before the cap drops them.
- **Trim the blank end of each row.** Upstream's later versions do it. It saves nothing at 80 columns and about
  45% at 200 columns with sixty-character lines.
- **Copy `vt100` into this repository.** Its `screen.rs` is 1354 lines, over the 800-line limit for a new
  file, and its files would be counted by the duplication scan.

## Decision

- **A fork, `soyeht/vt100-rust`, on its branch `panic-fixes` (tag `soyeht-0.16.2-2`).** It is upstream's
  `doy/vt100-rust` at the tag `v0.16.2` with five commits. The first, on the branch `compact-scrollback`
  (tag `soyeht-0.16.2-1`), packs the scrollback: a row that scrolls out of the screen is packed
  (`src/packed.rs`) into its text,
  the changes of its attributes and its wrap flag, in one allocation, without the blank end of the line, and is
  unpacked only while it is in view. The live screen keeps full cells. `visible_rows`, `Screen::cell` and
  everything else that reads rows return ordinary rows, so the public API and what every method returns are
  those of 0.16.2. The fork's `tests/scrollback.rs` checks every row of the history at every scroll position
  against the same output drawn on a screen too tall to scroll (every attribute and color, wide and combining
  characters, wrapped lines, rows with a background to the end of the line, eviction at the cap, output arriving
  while the view is in the history, a resize while it is), and `src/packed.rs` round-trips random rows. The
  upstream tests pass unchanged. The second commit fixes the panics that follow (below). The third makes the
  rows in view follow output incrementally: a row that scrolls out while the view is in the history used to
  unpack a whole screenful again for each line (12 us a line on a 50 row screen, against 1.3 us now and 1.1 us in
  the live view; unmodified 0.16.2 takes 0.8 us), and `tests/scrollback_view.rs` compares the view after every
  line with a full refresh. The fourth lets the panic fuzzer limit every screen to a few cells
  (`VT100_FUZZ_TINY`), and the fifth says in the fork's README what the panic fixes are.
- **The parser does not panic on any output.** Pty output is untrusted (any program, `cat` of a binary file, a
  remote shell), and `vt100` 0.16.2 panics on a few sequences: a line that wraps on a screen of one row, a
  character wider than the screen (a wide character on one column), a resize that cuts a wide character in two
  and the next write or erase on that row, and a size of zero. `ICH` with a count beyond the row (`CSI 65535 @`)
  also took seconds. The fork fixes each one at its cause, as upstream's later versions do for some of them
  (a character wider than the screen is ignored; a shrinking row clears the half of a wide character it cut).
  `tests/panic_fuzz.rs` generates operations from a seed (escape sequences of every kind, wide, combining and
  emoji characters, scroll regions, the alternate screen, resizes, scrollback offsets, random bytes), reads the
  screen back through every accessor after each one, and reduces a failing seed to a short list of operations:
  15.8% of 10,000 seeds panicked before the fixes (six places), none of 200,000 do after, and none of 100,000
  that keep every screen to 4 rows by 5 columns. A few seconds' worth of seeds run with `cargo test`.
  `TerminalScreen` still catches a panic of the parser and starts that screen again (`screen/contained.rs`),
  for the next one.
- **Pinned by revision.** `apps/desktop/Cargo.toml` and `apps/android/Cargo.toml` point `vt100` at the fork
  with `[patch.crates-io]` and a full `rev`; their lockfiles carry it. `apps/desktop/Cargo.toml` keeps
  `vt100 = "0.16.2"` as the requirement of its workspace; `apps/android/Cargo.toml` has none, since it gets
  `vt100` through `soyeht-client`, and its `[patch]` replaces that dependency. The engine workspace takes the
  fork too, with the same `[patch.crates-io]` and `rev` (`engine/Cargo.toml`, `engine/Cargo.lock`), for the fix
  of the panics on pane output alone: its screens have no scrollback.
- **The engine parses pane bytes in two places, neither under a lock.** `terminal_projection.rs` (the experiment
  recorder) runs on its own `terminal-observation-projector` thread, which holds no `Mutex` while the parser
  works; a panic there ends that thread, the capture reports a stopped worker, and the thread's join logs
  `engine.terminal_capture_panicked`. The usage probes (`usage/terminal.rs`) run on a worker thread of a refresh,
  off the Core lock (ADR 0079), under a turn that is a lock on a directory, not a `Mutex`; a panic ends that
  provider's read, which the commit releases and logs. The probe also catches a panic of the parser
  or of the source's driver around its whole read loop (`soyeht_platform::crash::caught`) and goes on to stop
  the child and remove the scratch directory, which the unwinding would have skipped (`Child` has no `Drop`). No lock is poisoned by a parser panic. The window's `TerminalScreen` catches a
  panic of the parser on every call that reaches it: writing, resizing, moving the view, selecting and reading
  rows (`screen/contained_reads.rs`). A read that only borrows the screen shows nothing until the next write,
  resize or scroll starts the screen again.
- **The 10,000 lines stay.** `scrollbackLines` is not touched.

## How we track upstream

- The fork's README says what changed and why, and the changes are small edits (`grid.rs`, `row.rs`,
  `screen.rs`, accessors in `cell.rs`), one new file (`src/packed.rs`) and new tests, so they rebase on an
  upstream tag as five commits (the last two only touch the fuzzer and the README). Upstream's later versions
  fixed some of the same panics; the rebase drops what they made redundant, and `tests/panic_fuzz.rs` says
  whether any is left.
- Upstream is at 0.18 and has changed the API (`Callbacks::on_scroll`, a non-zero `Parser::set_size`, and its own
  trimming of scrollback rows). Moving to it is a separate change of the window's code. When it is made, the
  fork's branch is rebased on that tag first, its tests are run, the `rev` is bumped, and `cargo test` of
  `apps/desktop` and the example above are run.
- Whoever reads the release notes of upstream at a release checks for a security fix in a version we do not
  have; one is taken by the same rebase.
- The change is offered upstream. If upstream takes a compact scrollback, the fork and the `[patch]` are
  removed in the same change that moves to that version.

## Consequences

- Measured with the same example and the fork, one pane with its history full takes 1.6 MB at 80 columns, 1.4 MB
  at 120 and 1.6 MB at 200 (27, 40 and 71 MB before), plain or coloured. The same parser fed 200,000 lines on the
  Linux machine, outside the window, went from 27, 40 and 64 MB to under 4 MB.
- Scrolling costs about the same CPU: feeding a million lines of output took 10 to 15% more CPU time on macOS
  and a third of the time on Linux (the old rows were 6 KB allocations that were freed one by one), and a view in
  the history unpacks only the rows it shows.
- A build fetches `vt100` from GitHub instead of crates.io, at a revision a lockfile pins. The dependency is
  review-required (`Cargo.toml`, `Cargo.lock`).
- We carry a fork. It is public, MIT-licensed like upstream, and holds nothing that is not upstream's code and
  this change.
