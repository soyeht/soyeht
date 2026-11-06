# ADR 0144: The Linux and Windows window and the Android core use a vt100 fork with a compact scrollback

- Status: accepted
- Date: 2026-11-05

## Context

The window for Linux and Windows (ADR 0084) and the Android core (ADR 0134) turn the engine's terminal bytes
into styled rows with the `vt100` crate. Each terminal pane keeps a parsed screen, for every pane of every
workspace, with `terminal.scrollbackLines` of history: 10,000 lines (`contracts/protocol.json`, `x-clientPolicy`).

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

- **Fewer lines.** It changes what the person can scroll back to. `terminal.scrollbackLines` stays at 10,000
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

- **A fork, `soyeht/vt100-rust`, on its branch `compact-scrollback`.** It is upstream's `doy/vt100-rust` at the
  tag `v0.16.2` with one change: a row that scrolls out of the screen is packed (`src/packed.rs`) into its text,
  the changes of its attributes and its wrap flag, in one allocation, without the blank end of the line, and is
  unpacked only while it is in view. The live screen keeps full cells. `visible_rows`, `Screen::cell` and
  everything else that reads rows return ordinary rows, so the public API and what every method returns are
  those of 0.16.2. The fork's `tests/scrollback.rs` checks every row of the history at every scroll position
  against the same output drawn on a screen too tall to scroll (every attribute and color, wide and combining
  characters, wrapped lines, rows with a background to the end of the line, eviction at the cap, output arriving
  while the view is in the history, a resize while it is), and `src/packed.rs` round-trips random rows. The
  upstream tests pass unchanged.
- **Pinned by revision.** `apps/desktop/Cargo.toml` and `apps/android/Cargo.toml` point `vt100` at the fork
  with `[patch.crates-io]` and a full `rev`; their lockfiles carry it. Both manifests keep `vt100 = "0.16.2"`
  as the requirement. The engine workspace stays on crates.io: its screens have no scrollback.
- **The 10,000 lines stay.** `terminal.scrollbackLines` is not touched.

## How we track upstream

- The fork's README says what changed and why, and the change is three small edits (`grid.rs`, accessors in
  `cell.rs` and `row.rs`) and one new file, so it rebases on an upstream tag as a single commit.
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
