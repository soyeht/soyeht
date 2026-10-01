# Contract 09 · Editor (code pane)

Status: CLOSED on 2026-09-11 (feature list decided). Design: screen 11 in
`design/soyeht-desktop.pen`. Colors are provided separately and must not be
changed here.

## 1. What it is
A third pane type next to "terminal" and "agent": `mode: "editor"`. It opens
files from the workspace worktree. The engine owns the file (reads, writes,
watches the disk, keeps history); the app renders and edits.

## 2. Non-negotiable requirement: speed
- Native Swift rendering, no web view. Text and highlighting are drawn per
  visible viewport, never for the whole file at once.
- Targets measured in CI on the reference Mac: open a 100k-line file in
  < 150 ms; keystroke-to-pixel latency < 16 ms (one frame); 60 fps scrolling
  in a 100k-line file; project search over 1 GB in < 1 s.
- Large files are handled in pieces (rope or piece table); highlighting is
  incremental (tree-sitter); never reparse the whole file per keystroke.

## 3. v1 features (closed list)
**Navigation**
1. Command palette (`Cmd+Shift+P`) and open file by name (`Cmd+P`), fuzzy.
2. "Search everywhere" (double Shift): files, symbols, commands and settings
   in one box.
3. Project-wide search with replace, results grouped by file, click goes to
   the line.
4. Go to definition (`Cmd+click`) and find references.
5. File symbol list (functions, types) inside "search everywhere".
6. Go to line N.

**Editing**
7. Syntax highlighting (tree-sitter) for Rust, Swift, TypeScript, Python,
   JSON, Markdown and shell; plain text for everything else.
8. Multi-cursor: `Cmd+D` selects the next occurrence, `Opt+click` adds a cursor.
9. Auto-close brackets and quotes, auto-indent, toggle line comment (`Cmd+/`).
9a. Undo and redo (`Cmd+Z`, `Cmd+Shift+Z`, Vim `u`) over an engine-owned,
    per-file history shared by every paired client; paste, cut, drag and drop
    and input-method composition reach the engine like typing (ADR 0071).
10. VS Code keybindings by default.
11. Vim mode (optional, off by default): normal/insert/visual, common motions
    and operators; ex commands limited to `:w`, `:q`, `:e`, `:%s`.

**Language**
12. Inline errors and warnings (underline plus hover message) from language
    servers (LSP): rust-analyzer, sourcekit-lsp, the TypeScript language
    server and pyright, one per language, launched and supervised by the
    engine (under the process supervisor, like any pane process).
13. The same LSP provides go to definition, references and the symbol list.

**File and disk**
14. Autosave (500 ms debounce); unsaved text survives engine and app restarts
    (draft stored in the database).
15. Reload when the file changes on disk (engine watcher): the view follows
    and marks the changed lines; if you changed the file too, show a conflict
    with both versions, never overwrite silently.
16. Local history for every file, even without git: every write becomes a
    version in the engine, with who wrote it (you or which agent); "restore
    this version".
17. Markdown preview beside the file.

**Git and diff**
18. Gutter bars: added, changed and deleted lines relative to git.
19. Side-by-side diff and per-line blame.

**The agent as a participant**
20. Who edited: every hunk written by an agent is marked with its name (gutter
    and a chip such as "aria · 2 min ago"); while the agent edits, its cursor
    is shown live.
21. Review hunk by hunk: the agent's diff shows accept and reject per hunk;
    reject reverts only that hunk.
22. A comment on a line goes straight to the owning agent pane as a change
    request (same mechanism as CI feedback in contract 03 §6), and the task
    card switches to "system's turn".
23. File paths become links everywhere: agent conversation, terminal output,
    CI errors; clicking opens the editor at that line (and column).

## 4. Out of v1
Code completion, refactoring (mass rename, extract method), debugger,
minimap, code folding, dark theme (colors come separately), human-to-human
collaboration, extensions.

## 5. Changes to other contracts
- 08 (engine and clients): methods `file.open|read|write|watch|history|
  revert|undo|redo|search|symbols|definition|references|diagnostics|diff|format`,
  `editor.review.accept|reject|comment`, `lsp.status`; events
  `file.changed_on_disk`, `file.saved {by}`, `file.conflict`,
  `lsp.diagnostics`, `editor.agent_cursor`, `editor.review_requested`.
- 07 (persistence): unsaved drafts and per-file version history in the
  database, capped per file (for example 200 versions or 50 MiB) with a
  clear button.
- 03 (tasks): an editor comment is a change request on the task.
- 05 (pane state): agent file edits feed the state phrase ("editing screen.rs").

## 6. Place in the slice plan
Slice 4, "Editor and app bar", right after tasks (3), together with
contract 10. Slice 5 is the browser (contract 11); the earlier slices shift
after it: 6 harness switch · 7 raw terminal and the experiment · 8 full
iPhone · 9 Linux and Windows · 10 production.
Order inside the slice: buffer + highlighting + save/reload/history →
palette/open/search → LSP (definition, references, symbols, diagnostics) →
diff/git → agent (marks, review, comments, links) → Markdown → Vim → app bar
and usage rings.
