# ADR 0155: A terminal is fed once per frame, and drawn less often while it floods

- Status: accepted. Extends ADR 0012 (the native client uses SwiftTerm's `TerminalView`) and ADR 0145.
- Date: 2026-11-08

## Context

One shell pane running `while true; do ls -laG /usr/bin; done` kept the Mac app at a whole core, and four
of them made the engine close the connection with `slow_consumer` (its queue for a client is 8 MiB, `docs/contracts/08-engine-and-clients.md`)
several times in thirty seconds, so the app resynchronized again and again. A sample of the main thread,
which was never idle, showed where the time went: 44% drawing the terminal (a scrolling screen changes every
row, and shaping the text of a row with CoreText, whose ligature lookups are expensive in the mono face, is what
a redraw costs, at sixty frames a second), 27% parsing the bytes (a third of that packing the lines that leave
the screen, ADR 0145), and the rest the connection and SwiftUI. The chunks of output arrived one by one, and
each one was fed to the emulator by itself.

## Decision

- **`TerminalOutputCoalescer` feeds a pane once per frame.** The chunks of one pane wait for
  `ClientPolicy.terminalOutputFlushMs`, are joined in their order and fed as one. A replay (`reset`) starts a
  new batch, since the screen is replayed from its beginning there. The coalescer lives in `apps/shared`; the
  Mac surface owns one.
- **A pane that floods is fed and drawn less often.** When a frame carried
  `ClientPolicy.terminalOutputFloodBytes` or more, the next one waits `terminalOutputFloodMs` (about twenty
  frames a second). The bytes and their order are the same; only the number of redraws falls. The interval
  returns to one frame when a frame carries less.
- **Nothing is delayed that the person feels or that would grow without bound.** Output within
  `terminalInteractiveWindowMs` of a key is fed as it arrives (the echo), and a backlog of
  `terminalOutputFlushBytes` is fed at once. The surface feeds what waits before the grid changes, and no byte
  is dropped.
- **No view observes the stream positions.** `EngineClient.terminalStreams` is not observed any more: it moves
  with every chunk, and the pane card that reads a gap notice was built again for each one, for every pane.
  The notices are `terminalGapTexts`, which changes when a gap does.
- **A surface keeps the colors it has.** Setting a color makes the emulator drop its attribute caches and mark
  every row dirty, so `TerminalSurface.applyTheme` sets them only when the palette changed.

The numbers are `x-clientPolicy.terminal` in `contracts/protocol.json`.

## Consequences

- The Mac client does one feed per frame, and a flooding pane is drawn at about a third of the old rate. On
  VM1 (`ls -laG /usr/bin` in a loop, the app in Release, three runs each), one pane went from 64% to 43% of a
  core, with the same output delivered; four panes were disconnected 10 to 18 times in thirty seconds before and
  never after, and six panes 6 times before and never after. Four flooding panes still cost about 85% of the main
  thread, mostly parsing, which is why a stall of a few seconds can still overflow the engine's queue.
- A pane that prints a lot shows its screen up to 50 ms late. Typing is not delayed.
- The iPhone and iPad surfaces feed each chunk as before; they share the policy numbers when they adopt the
  coalescer.
- Parsing still costs what it costs per byte. Caching the shaped lines of the screen, which would make a
  redraw of a scrolling screen cheap, belongs to the SwiftTerm fork.
