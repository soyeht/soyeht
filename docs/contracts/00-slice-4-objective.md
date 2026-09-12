# Objective · New Soyeht slice 4: the editor and the app bar

Self-contained, for execution to completion without questions to Caio. Same
absolute safeguards as slices 1–3 (`00-slice-1-objective.md` §0 and §5: no
production, Dev profile `Soyeht.next/dev`, no secrets, no tags, no competitor
names, no assistant attribution, Apache-2.0). Same autonomy rule: ambiguity
becomes an ADR and work continues; self-merge with the green local gate;
victoria reviews after each merge. Date: 2026-09-12. Caio chose one whole
objective (option B), executed as many small PRs to completion in goal mode.

Keeps the hardening round 1 ratchets green: all new persistence lives in
`soyeht-store` as named functions with real-database tests (no SQL in the
engine); new engine effects go through ports with test doubles; no panic in
production paths; one error enum per crate mapped once to the wire; no
production function above 120 lines; no test, acceptance or gate step removed.
The four ratchets, the restricted-vocabulary scan and the Mac text-font gate
run before every merge.

**Design fidelity is a first-class acceptance** (standing rule from
`00-design-alignment-1-objective.md` §5). Every Mac screen this slice builds —
the editor pane (Tela 11) and the app bar with its usage rings (Tela 12, 12b) —
is reproduced faithfully against its `.pen` frame using the shared `MacTheme`
tokens, and victoria compares the running app to the `.pen` render before the
screen is accepted (see §2). The `.pen` is consumed, never changed.

## 0. Governing contracts for this slice
`09-editor.md` (entire document: the `editor` pane, the non-negotiable speed
requirements in §2, the closed v1 feature list in §3, the protocol changes in
§5, the in-slice order in §6) and `10-app-bar-and-usage.md` (entire document:
the 56 px rail and client-side view switching in §1, the usage rings in §2, the
read-only per-provider readers and their sources in §3, the protocol in §4, the
order in §7). Cross-contract changes these two require: `08-engine-and-clients.md`
(the new `file.*`, `editor.*`, `lsp.*`, `usage.*` methods and events),
`07-persistence.md` (unsaved drafts and per-file version history, capped),
`03-workspace-tasks.md` §6 (an editor line comment is a change request on the
owning task), `05-pane-state.md` (agent file edits feed the state phrase).
Design: `design/soyeht-desktop.pen` Screen 11 (editor) and Screens 12 and 12b
(app bar and the usage detail balloon). Reuse the ACP agent pane, the task
board and the terminal panes from slices 1–3 unchanged.

## Architecture and maintainability (decided first, before any feature)
This is the largest slice and it ships as one goal, so its structure is chosen
up front, in an ADR that is the first PR of the slice, not left to emerge.
Requirements, in the spirit of the hardening round:
- **Bounded subsystems, one responsibility each, no reaching into another's
  internals.** The editor engine (rope or piece table, viewport model,
  incremental tree-sitter highlighting) is a self-contained, unit-tested unit,
  separate from the SwiftUI view that draws it. The LSP client (framing, one
  supervised server per language, diagnostics mapping) is its own module. File
  ownership (read, write, watch, history, conflict) is its own module. The
  usage readers are one registry with a reader trait and one file per provider.
  Git and diff is its own module.
- **Every external effect goes through a port/trait with a test double**
  (hardening item 2): the language-server process, file IO and the disk
  watcher, git, and each usage source. CI runs against the doubles; only the
  real implementation touches the outside.
- **One error enum per crate**, named variants, mapped once to the wire
  (hardening item 4). New Rust code lives in the smallest sensible crate, and a
  large independent subsystem (the editor engine, the usage registry) becomes
  its own crate rather than a god module bolted onto an existing one.
- **Single source of truth**: the protocol schema generates every wire type;
  the `MacTheme` tokens are the only Mac style source; no value is duplicated
  by hand across Rust, Swift and TypeScript.
- Functions stay under 120 lines, no SQL outside the store, no panic in
  production — the existing ratchets. The boundary ADR names the crates and
  modules and their ports so victoria can review each PR against a decided
  architecture, not a moving one.

## 1. What to build
1. **Protocol** (`contracts/protocol.json`, with vectors and generated types
   for Rust, Swift and TypeScript): `pane.create` gains `mode: "editor"`; add
   the file methods `file.open`, `file.read`, `file.write`, `file.watch`,
   `file.history`, `file.revert`, `file.search`, `file.symbols`,
   `file.definition`, `file.references`, `file.diagnostics`, `file.diff`,
   `file.format`; the review methods `editor.review.accept`,
   `editor.review.reject`, `editor.review.comment`; `lsp.status`; and the usage
   methods `usage.list`, `usage.refresh`. Events: `file.changed_on_disk`,
   `file.saved {by}`, `file.conflict`, `lsp.diagnostics`, `editor.agent_cursor`,
   `editor.review_requested`, `usage.changed {provider, …}`. The
   `usage.list`/`usage.refresh` shapes are fixed by contract 10 §4. No slice 1–3
   method, event or field changes.
2. **`soyeht-store`**: unsaved editor drafts and per-file version history
   (contract 07), capped per file (200 versions or 50 MiB, whichever first,
   with a clear operation), each version carrying its author (you or which
   agent); the last usage reading per provider so rings show a value right
   after restart, marked stale. Named functions, real-database tests, one
   migration with a migration test.
3. **`soyeht-engine`, file ownership**: the engine owns each open file — read,
   write, disk watch, history, conflict detection — behind ports with doubles.
   Autosave with a 500 ms debounce; a draft survives engine and app restart; a
   disk change the engine did not make raises `file.changed_on_disk` and, if the
   buffer also changed, `file.conflict` with both versions, never a silent
   overwrite. Project search and symbols. The agent-as-participant path: an
   agent's write is attributed (`file.saved {by}`), a line comment becomes a
   change request on the owning task (contract 03 §6, "system's turn"), and an
   agent edit feeds the pane state phrase (contract 05). File paths in agent
   output, terminal output and CI feedback resolve to an editor open at
   line and column.
4. **LSP under the supervisor**: one language server per language —
   rust-analyzer, sourcekit-lsp, the TypeScript language server, pyright —
   launched and supervised by `soyeht-ptyd` exactly like any pane process
   (contract 07), with reconnection and `lsp.status`. The engine speaks LSP and
   maps diagnostics, definition, references and the symbol list to the
   `file.*` methods and `lsp.diagnostics` events. CI uses a deterministic fake
   language server in the repo; a local test may use the real servers.
5. **Usage readers** (`soyeht-engine`, read-only, one registry, one reader per
   provider, behind a port): each reader reads only what the provider's own
   tools already keep on this Mac, never stores a credential, never refreshes or
   rotates a token, and never sends a token anywhere but that provider's own
   usage endpoint, with a preferred source and ordered fallbacks (contract 10
   §3). **First task of the slice: run each reader against a real login on
   Caio's Mac and record the result, source by source, in `docs/reports/` —
   before the rings ship.** `usage.list`/`usage.refresh`/`usage.changed` with
   the states and rate limits of contract 10 §4; refresh cadence, backoff and
   the stored-last-reading rule of §3. CI uses fake readers with scripted
   states (ok, stale, login_needed, no_access, unavailable). No provider glyph
   without an MIT-licensed icon set with its notice in `apps/shared` (§6); no
   ring for a signed-in provider with no reader.
6. **Native editor pane, Mac** (Tela 11, non-negotiable speed §2): Swift
   rendering, no web view; text and highlighting drawn per visible viewport,
   never the whole file; a rope or piece table for the buffer; incremental
   tree-sitter highlighting (Rust, Swift, TypeScript, Python, JSON, Markdown,
   shell; plain text otherwise), never a full reparse per keystroke. The v1
   feature list of contract 09 §3: command palette and open-by-name, "search
   everywhere", project search and replace, go to definition and references,
   symbol list, go to line; multi-cursor, auto-close and auto-indent, toggle
   comment, VS Code keybindings, optional Vim mode; inline LSP diagnostics;
   autosave, disk reload with conflict, local history with restore, Markdown
   preview; git gutter bars, side-by-side diff, per-line blame; agent hunk
   marks with a live agent cursor, hunk-by-hunk accept/reject, a line comment
   that becomes a task change request, and clickable file-path links. Match
   Screen 11 against `MacTheme`.
7. **App bar, Mac** (Tela 12, 12b, contract 10): the 56 px left rail, hidden by
   default, toggled by the top-bar button and `Cmd+Shift+B`; icon buttons
   Agents · Editor · Tasks · Terminal · iPhone, a separator, the usage rings, a
   spacer, Settings; `Cmd+1`…`Cmd+5` switch views. View switching is
   client-side only and never creates, closes or focuses an engine pane. Rail-open
   is a per-client preference (contract 07), never engine state. The usage rings
   of §2: one per provider with a reader, the provider glyph inside and the
   percentage below, band colors from the tokens (`success`/`warning`/`danger`),
   a dimmed "—" for no or stale reading, nothing for a signed-in provider with
   no reader; the hover detail balloon (Screen 12b) and the Settings detail.
   Match Screens 12 and 12b against `MacTheme`.
8. **iPhone**: the usage rings in Settings, reading the same `usage.list`
   (contract 10 §4). No editor on the phone in this slice.
9. **CLI**: `pane create --editor <path>`, `file open|read|write|history|
   revert|search|symbols|diff`, `usage list|refresh`, for scripting and e2e.
10. **CI**: everything from slices 1–3 plus this slice's tests; the speed
    targets of contract 09 §2 measured on the reference Mac (100k-line open
    < 150 ms, keystroke-to-pixel < 16 ms, 60 fps scroll, 1 GB search < 1 s),
    each a gate step that fails on regression; e2e with the fake LSP and the
    fake usage readers under launchd.

## 2. Acceptance (work ends only when all pass)
1. [CI] Open an `editor` pane on a file: it renders by viewport; a 100k-line
   file opens under the §2 target and scrolls at 60 fps; a keystroke reaches
   pixels within one frame; highlighting is incremental. The targets are gate
   steps that fail on regression.
2. [CI] Edit and autosave: the file is written after the 500 ms debounce with
   `file.saved`; killing the engine and app mid-edit and restarting restores the
   unsaved draft; a version with its author is recorded and "restore" brings it
   back.
3. [CI] Disk conflict: an external change to an unmodified file reloads and
   marks the lines; an external change to a modified file raises `file.conflict`
   with both versions and never overwrites silently.
4. [CI] Navigation and language: the palette, open-by-name, search everywhere,
   project search/replace, go to definition, references, the symbol list and go
   to line work; the fake LSP's diagnostics underline with a hover message;
   `lsp.status` reflects the supervised server; a killed language server is
   restarted by ptyd and diagnostics resume.
5. [CI] Git and diff: gutter bars mark added/changed/deleted lines against git;
   side-by-side diff and per-line blame render.
6. [CI] The agent as participant: a fake agent's write is attributed with its
   name and a live cursor; hunk accept/reject reverts only the chosen hunk; a
   line comment arrives at the owning agent pane as a change request and the
   task card shows "system's turn"; a file path in agent or CI output opens the
   editor at the line and column.
7. [CI] Vim mode off by default; when on, normal/insert/visual, common motions
   and operators, and `:w` `:q` `:e` `:%s` work; turning it off restores the VS
   Code keybindings.
8. [CI] Usage: `usage.list` returns one entry per fake provider with the
   contract-10 states; the rail draws a ring per reader with the glyph inside
   and the percentage below, the band color by threshold, a dimmed "—" for
   stale or missing, and nothing for a signed-in provider with no reader;
   `usage.refresh` is rate-limited to one per provider per 30 s; a stored
   reading shows right after restart marked stale.
9. [CI] The app bar: the rail toggles with the button and `Cmd+Shift+B`,
   `Cmd+1`…`Cmd+5` switch views without creating, closing or focusing any engine
   pane, and rail-open persists per client across restart without touching
   engine state.
10. [reviewer] Design fidelity: victoria compares a screenshot of the running
    Mac editor (Tela 11) and app bar with rings (Tela 12, 12b) against a render
    of the matching `.pen` frame and confirms palette, spacing scale, type
    ramp, radii, elevation and each component's shape and placement; a
    divergence on any axis fails the screen. The Mac text-font gate stays
    green. The closing report pastes the image pairs.
11. [CI] No slice 1–3 functional acceptance or test regresses; the four
    hardening ratchets, the restricted-vocabulary scan and the Mac text-font
    gate are green; the only protocol changes are the additions in §1 item 1.
12. [local, procedure] On Caio's Mac: each usage reader run against a real
    login, recorded in `docs/reports/` before the rings ship (§1 item 5); the
    editor opened on a real repository file with the real language servers,
    editing, autosave, diagnostics, diff, agent review and a Markdown preview
    exercised by hand.

## 3. Outside this slice
The browser (slice 5); harness switching (slice 6); the raw-terminal detector
experiment (slice 7); the full iPhone board and push (slice 8); Linux and
Windows (slice 9); production (slice 10). From contract 09 §4: code completion,
refactoring, debugger, minimap, code folding, dark theme, human-to-human
collaboration, extensions. From contract 10 §5: cost in currency, per-pane
token counts, a menu-bar widget, low-usage alerts.

## 4. Workflow and delivery
Order (contracts 09 §6 and 10 §7): app-bar rail and view switching → usage
readers investigation and `usage.list` → rings and balloon and Settings detail
→ editor buffer, viewport rendering and highlighting → save, reload, history
and drafts → palette, open and search → LSP (definition, references, symbols,
diagnostics) → git and diff → the agent (marks, review, comments, links) →
Markdown → Vim → the on-device procedure. Small PRs, green local gate with the
real restricted vocabulary, the four hardening ratchets and the Mac text-font
gate, self-merge; an ADR for each ambiguity; each Mac screen carries the design
fidelity acceptance. On completion, `docs/reports/slice-4.md` with the PR
links, the reader-source findings and the design image pairs, and a message to
victoria. victoria reviews each PR for correctness, contract preservation and
design fidelity.
