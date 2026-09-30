# Multi-terminal client cost

Measured for issue #350 on 2026-09-18. The purpose is to bound the client cost
of keeping every raw terminal subscribed and every visible terminal rendered,
not to establish a release performance budget.

## Setup

- VM: rewrite VM1, macOS 26.4, 8 virtual CPUs.
- Client build: `715a8f0`, app PID `92262`, binary timestamp 09:44:46 BRT.
- Window: 1024 logical points wide, one visible workspace.
- Pane counts: 1, 2, and 4 visible raw terminals; then 8 raw terminals with
  four visible and four in a second, non-visible workspace.
- Idle sample: `top -l 31 -s 1 -pid 92262 -stats pid,cpu,mem` (30 seconds).
- Burst: 5,000 equal-length lines per pane, written concurrently to each PTY
  slave in 100 chunks of 50 lines with 20 ms between chunks. CPU was sampled
  once per second while the finite burst ran. The remote writer took 3.1–3.5 s.

The 8-pane case deliberately separates rendering from transport. A pane in a
non-visible workspace has no SwiftTerm surface mounted, but it is still
subscribed. "Not rendered" does not mean "not subscribed": the engine log
contains one `pane.stream.subscribe` for each of the eight pane IDs, and the
client continued accepting and sequencing bytes for the four hidden panes.

## Results

| Panes | Visible SwiftTerm surfaces | Idle CPU (31 samples) | Idle RSS shown by `top` | Burst CPU samples after baseline | Peak |
| ---: | ---: | ---: | ---: | --- | ---: |
| 1 | 1 | 0.0% | 66 MB | 44.0, 44.6, 43.8, 0.0% | 44.6% |
| 2 | 2 | 0.0% | 56 MB | 48.4, 50.2, 50.1, 13.3% | 50.2% |
| 4 | 4 | 0.0% | 63 MB | 84.1, 87.0, 85.6, 10.9% | 87.0% |
| 8 (4 hidden) | 4 | 0.0% | 56 MB | 4.7, 103.5, 104.0, 102.9, 100.0% | 104.0% |

The idle CPU result is stable: every sample was zero at all four counts. The
burst scales with bytes routed and visible surfaces, reaching about one CPU core
for eight simultaneous streams. It returned to 0.0% immediately after the
finite burst. The RSS readings are intentionally reported as observed rather
than converted into a per-pane slope: after repeated scrollback bursts, macOS
reclaimed/compressed pages and the samples were non-monotonic.

No stream gap, reference mismatch, or binary-frame error was present in the
rewrite logs after the four runs.

## Hands-on checks from the same run

- Two terminals painted independent prompts and output at the same time.
- Clicking the body of the unselected lower terminal moved keyboard input to
  it; `echo lowerpane` appeared only there.
- Mouse dragging selected text in the terminal under the pointer.
- Closing the first terminal in a two-leaf column kept the later terminal's
  scrollback, including `QA350_SCROLLBACK_SURVIVES`, while it expanded into the
  promoted space.
- Switching to the second workspace and back preserved the visible terminal's
  content and surface state.

