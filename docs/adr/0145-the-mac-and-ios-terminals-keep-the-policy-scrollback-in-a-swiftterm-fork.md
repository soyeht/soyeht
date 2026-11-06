# ADR 0145: The Mac and iOS terminals keep the policy scrollback, in a SwiftTerm fork that stores it compactly

- Status: accepted. Extends ADR 0012 (the native client uses SwiftTerm's `TerminalView`).
- Date: 2026-11-06

## Context

`x-clientPolicy` says a terminal keeps 10,000 lines of history (`ClientPolicy.terminalScrollbackLines`), and
the desktop client does. The Mac, iPhone and iPad terminals kept SwiftTerm's own default of 500 lines because
its memory was not measured. Measured, on a terminal fed 13,000 full-width lines with 10,000 of scrollback
(malloc bytes in use): **21.0 MB at 80 columns and 50.4 MB at 200 columns** for one pane, because SwiftTerm
stores every cell of every line as a 24-byte `CharData`, whatever it holds. Ten panes of 200 columns are 0.5 GB.
Most of it is ASCII text, a few attributes and blanks.

Keeping 500 lines would leave the Mac and the phone with a shorter history than the same session has on the
desktop client. Keeping 10,000 as SwiftTerm stores them costs too much memory.

## Decision

- **Both surfaces start with the policy's scrollback.** `TerminalOptions.soyehtStartup` is SwiftTerm's default
  options with the scrollback set to `ClientPolicy.terminalScrollbackLines`; `TerminalSurface` (Mac) and
  `PhoneTerminalSurface` (iPhone, iPad) are made through a `convenience init(font:)` that passes it to
  SwiftTerm's `init(frame:font:options:)`. A reset keeps the options of the terminal it resets, so this is
  the one place the number enters. `TerminalScrollbackTests` fails when a surface differs from the policy.
- **SwiftTerm is a fork that packs the lines of the scrollback.** `soyeht/SwiftTerm`, a fork of
  `migueldeicaza/SwiftTerm` with its licence and history, at upstream v1.20.0 plus one change: a line that
  leaves the screen is stored as run-length coded characters and runs of attributes (every field of `CharData`
  and `Attribute`, hyperlink atoms and semantic roles included), and is unpacked, as a whole, by the first
  access to its cells. The buffer packs the oldest of the lines it unpacked last (`max (512, 4 * rows)`), so
  reading a long history one line after another, as a search or a select-all does, never holds more than that
  many full lines. Every reader of the buffer (drawing, selection, search, reflow, accessibility) reaches the
  cells through `BufferLine`, which unpacks on demand, so none of them changed. Measured the same way:
  **2.4 MB at 80 columns and 3.7 MB at 200 columns** (a ninth and a thirteenth); about 5 and 10 MB when
  every cell has a colour of its own, which no real output does. Feeding costs about 1.5 microseconds a line
  more. The fork's `CompactScrollbackTests` feed the same output to a terminal that packs and one that does not
  (wide and combining characters, true colour, hyperlinks, prompts, wrapped lines, the cap, resizes, selections,
  searches, history size changes) and compare every cell; the whole upstream suite passes as before.
- **The dependency is pinned to a tag of the fork**: `apps/shared/Package.swift` has
  `exact: "1.20.0-soyeht.2"` of `https://github.com/soyeht/SwiftTerm`, and the four `Package.resolved` files
  hold the revision of the tag. The Mac and iOS license step reads `checkouts/SwiftTerm/LICENSE`, which the fork
  keeps.
- **How the fork follows upstream.** Its `compact-scrollback` branch is an upstream release plus the change
  (`PackedLine.swift` and `BufferHistory.swift` are new files; `BufferLine`, `Buffer`, `Terminal` and `CharData`
  change by a few lines, listed in its README). To take a new release: fetch upstream, rebase the branch onto
  its tag, run `swift test`, tag `v<upstream>-soyeht.<n>`, and change the pin here with a pull request. A test
  in the fork fails when upstream adds a field to `CharData` or `Attribute` that the packing does not store.
  Upstream is not asked to take the change; if it does, the pin returns to upstream.
- **One fix of upstream is in the fork.** The fork's fuzz test (random output with margins, scrolling
  regions and resizes, packing on and off) found that Insert Line and Delete Line with left and right
  margins moved the rows from the cursor row for as many rows as the scrolling region has, so they read and
  wrote past the bottom margin and, after a resize, in lines of the old width left in the buffer's array:
  an out of range copy that a release build does not check, reachable from the bytes a process writes. They
  now move the rows from the cursor to the bottom margin, and the copy cuts its range to the line.
  `1.20.0-soyeht.1` does not have it; the pin is `.2`.

## Consequences

- A pane of 10,000 lines costs a tenth of what it would, so ten panes of 200 columns are about 40 MB and not
  0.5 GB. The history is the same on the Mac, the phone and the desktop client.
- Reading the history changes the line it reads (it unpacks it), as the library did not before. Like the rest
  of SwiftTerm, a terminal is not safe to read from one thread while another feeds it.
- A resize that reflows the history unpacks every line once and packs them again at the end: a spike of the
  old size for the length of the resize. It is not a steady cost, and it is not measured here.
- A dependency on a fork is a thing to keep: a security fix upstream needs a rebase and a new tag. The change is
  small enough to rebase in one sitting, and the tests tell whether it still holds.
