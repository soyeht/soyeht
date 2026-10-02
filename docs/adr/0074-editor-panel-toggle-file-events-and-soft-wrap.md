# ADR 0074: Project panel toggle, file events and soft wrap

- Status: accepted
- Date: 2026-10-02

## Context

The owner wants the code editor simple and functional. Completion and hover
come from the LLM agents, so LSP completion and hover are out of scope
(decision of 2026-10-02). Three everyday gaps remained:

- In a narrow window `PaneLayoutFit` folds the project panel away (ADR 0072),
  and nothing on screen brings it back.
- The engine polled every folder a client showed once a second, even when
  nothing changed.
- Long lines could only scroll sideways.

## Decision

**Project panel toggle.** The editor header has a sidebar button, and View >
Toggle Project Panel has Cmd+Shift+E, Zed's project panel key. No other
Soyeht binding uses it. A toggle stores a per-workspace choice
(`EditorProjectPanelPreference`, kept in `EditorInputPreferences`). Before
the first toggle the panel is `automatic`, as in ADR 0072. A toggle hides a
visible panel (`hidden`), or else keeps it open (`shown`). `PaneLayoutFit`
takes the choice:

- `shown` tries every presentation at full floors first: the stored
  arrangement, one row, one column, then tab groups. Only then does it try the
  compact ones, so the panel is folded only when not even a lone editor with
  the panel fits the window.
- `hidden` lays out at compact floors only.

The divider gestures clamp against the same fit. The editor shows the panel
only when the person allows it and its pane is at least its full floor, so
the panel is never clipped.

**File events.** `project.tree.watch` starts one recursive watcher per watched
workspace through the `notify` crate: FSEvents on macOS, inotify on Linux.
`notify` is a new dependency. Nothing in `Cargo.lock` watched files. It is the
maintained crate that rust-analyzer and watchexec use.
We take only its `macos_fsevent` feature, which adds `fsevent-sys`,
`notify-types`, `inotify`, `kqueue` and `walkdir`, and no async runtime. The
callback only queues the event. The engine loop drains the queue and maps each
path to the folder that lists it and to the path itself. When no event has
arrived for 75 ms, or the burst has lasted 500 ms, it reads only the watched
folders that were touched. It announces a folder only when its listing
fingerprint changed, so the protocol events are unchanged. A rescan flag, an
error or a lost stream marks every folder of the workspace. Where no watcher
starts, as when inotify runs out of watches, that workspace falls back to the
old one-second polling. Watchers stop when no connection shows the workspace.

**Soft wrap.** View > Soft Wrap (Option+Z) is one global choice, like Zed's
`soft_wrap` setting. Zed binds Cmd+K Z, a chord a menu item cannot hold.
Option+Z is VS Code's Mac key. Wrapping lives in shared code:

- `EditorWrapLayout` counts rows per page line by character cells: a tab
  runs to the next stop of four cells, combining marks take no cell, and
  example hang at the end of a row, as in AppKit. It also maps document rows
  to logical lines for paging.
- `EditorTextLayout` gives the Mac text view a container exactly `columns`
  cells wide, and a paragraph style that wraps by character, with matching
  tab stops. AppKit therefore breaks lines where the shared layout does. A
  headless test checks this.
- Lines outside the loaded page count one row each. This changes only the
  scroll extent until that page loads.
- The gutter labels the first row of each logical line, at the position the
  text view laid out.
- Caret movement, find highlights, undo landings and clicks past a row's
  end use the text view's own layout of the page, so they stay right.
- Typing re-counts the rows of the loaded page, at most
  `editorViewportLines` lines. It replaces the line count it already did and
  never reads the whole file.

## Consequences

- A forced-open panel can rearrange a narrow window into a stack or tabs. That
  is the person's choice, and the stored layout is unchanged.
- The engine now does no folder work while the disk is quiet. A burst is read
  at most twice a second. Events from huge build folders are drained and
  dropped unless a watched folder lists them.
- Glyphs wider than one cell, such as CJK or emoji, can wrap a row earlier in
  AppKit than the shared count. The gutter still follows the drawn text, but
  the scroll extent can be a few rows off until the page reloads.
- Vim `j` and `k` keep moving by logical line.
