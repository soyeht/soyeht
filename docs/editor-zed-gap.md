# Editor gap analysis against Zed

Date: 2026-10-01. Scope: the everyday editing features of the code editor pane
(engine `engine/soyeht-editor`, `engine/soyeht-engine/src/editor`,
`engine/soyeht-engine/src/languages`; Mac UI `apps/mac/Sources/Editor*`; shared
client `apps/shared/Sources/SoyehtClient/Editor*`), compared with Zed's public
documentation at <https://zed.dev/docs>. Contract 09 (`docs/contracts/09-editor.md`)
is the closed v1 feature list; its section 4 puts completion, minimap and
folding out of v1.

Marks: **Have**: works end to end. **Partial**: exists with a material
limitation. **Lack**: absent. **Out of scope**: the owner decided not to build
it. The table reflects `main` before this change. The
last row group says what this change closes.

## Feature table

| Feature | Mark | Our evidence | Zed reference |
| --- | --- | --- | --- |
| File open, save, dirty state | Have | `file.open` (`editor/api.rs`); every accepted write persists a draft and autosaves 500 ms later (`editor/persistence.rs`, ADR 0047); header shows `· unsaved` / `· saved` / `· conflict` (`EditorPaneView.swift` `EditorPaneHeader`); disk changes reload or raise a conflict. No explicit `Cmd+S`: it is swallowed (`EditorTextView.performKeyEquivalent`). | [Autosave](https://zed.dev/docs/reference/all-settings#autosave) |
| Multiple tabs / buffers | Have | Engine-owned tab order and one preview tab per workspace (`editor/tabs.rs`, `editor.tabs.list|move`, ADR 0072); `EditorTabBar` closes with the hover `×`, `Cmd+W` or a middle click, reorders by drag, cycles with `Cmd+Shift+[` / `]`; closing saves the draft first. No `Ctrl+Tab` recent-order switcher and no pinned tabs. | [Editor tabs](https://zed.dev/docs/reference/all-settings#editor-tabs), [Preview tabs](https://zed.dev/docs/reference/all-settings#preview-tabs), [Tab Switcher](https://zed.dev/docs/tab-switcher) |
| Undo / redo | Have | Engine-owned per-file `EditHistory` (`soyeht-editor/src/history.rs`), `file.undo` / `file.redo` through the durable write path, `Cmd+Z`, `Cmd+Shift+Z`, Edit menu and Vim `u` (ADR 0071). The caret after a replay could keep its pre-undo position in the status bar (fixed by this change). | `editor::Undo` in [Vim: restoring common keybindings](https://zed.dev/docs/vim#restoring-common-text-editing-and-zed-keybindings) |
| Paste, cut, drag and drop, IME | Have | `EditorTextView.shouldChangeText` turns paste, cut and drop into verbatim engine edits; marked text stays local until the composition commits (ADR 0071). | Baseline text-editing behavior; see [Multibuffers](https://zed.dev/docs/multibuffers#editing-in-a-multibuffer) for paste across cursors |
| Multi-cursor | Partial | `Cmd+D` next occurrence and `Opt+click` add cursor (`EditorNativeViewport.swift`, `EditorEditing.nextOccurrence`); search covers only the loaded page; no add-cursor above/below, no select-all-occurrences. | [Multibuffers](https://zed.dev/docs/multibuffers#editing-in-a-multibuffer), [Multi cursor modifier](https://zed.dev/docs/reference/all-settings#multi-cursor-modifier) |
| Find and replace (in file) | Partial | Only AppKit's find panel (`usesFindPanel = true`), which sees the loaded page (at most `editorViewportLines`), not the file; replace through it bypassed the engine. Project-wide search and replace exist (`file.search`, palette `.project`, `EditorNavigationPalette.replaceAll`). | [Search settings](https://zed.dev/docs/reference/all-settings#search), `buffer_search::Deploy` in [Vim](https://zed.dev/docs/vim#restoring-common-text-editing-and-zed-keybindings), [Project search](https://zed.dev/docs/finding-navigating#project-search) |
| Go to line | Have | Palette mode `.line` (`EditorNavigationPalette.goToLine`) loads a page at the line; status bar shows `Ln, Col`. | `:<number>` in [Vim ex commands](https://zed.dev/docs/vim#ex-commands), [Line indicator format](https://zed.dev/docs/reference/all-settings#line-indicator-format) |
| Syntax highlighting | Partial | Incremental tree-sitter for Rust, Swift, TypeScript, Python, JSON, Markdown, shell (`soyeht-editor/src/syntax.rs`), viewport-only projection. Classification is a node-kind heuristic with a short shared keyword list, not per-language highlight queries, so many tokens stay uncolored; no semantic tokens; 7 languages. | [Syntax highlighting and themes](https://zed.dev/docs/configuring-languages#syntax-highlighting-and-themes), [Semantic tokens](https://zed.dev/docs/semantic-tokens) |
| Soft wrap | Have | Done (ADR 0074): View > Soft Wrap (`Cmd+Option+Z`), one global choice. Shared `EditorWrapLayout` counts rows per page line, and the Mac text container is exactly that many cells wide. Line numbers stay on logical lines. Typing re-counts only the loaded page. | [Soft wrap](https://zed.dev/docs/reference/all-settings#soft-wrap) |
| Line numbers and gutter | Have | `EditorLineGutter` draws numbers, the current line, git markers, agent review and agent cursors; no diagnostic or fold markers, no relative numbers. | [Gutter](https://zed.dev/docs/reference/all-settings#gutter), [Relative line numbers](https://zed.dev/docs/reference/all-settings#relative-line-numbers) |
| Indentation, auto-indent | Partial | Newline keeps the indent and adds four space characters after `{[(` (`EditorEditing.insertionPlan`); no Tab/Shift-Tab block indent, no tab size or hard-tab setting, no outdent on `}`, no indent guides. | [Auto indent](https://zed.dev/docs/reference/all-settings#auto-indent), [Tab size](https://zed.dev/docs/reference/all-settings#tab-size), [Hard tabs](https://zed.dev/docs/reference/all-settings#hard-tabs), [Indent guides](https://zed.dev/docs/reference/all-settings#indent-guides) |
| Bracket matching | Partial | Auto-close and type-over of `()[]{}""''` (`EditorEditing.closingCharacter`); no matching-bracket highlight, no jump to the match. | [Auto close](https://zed.dev/docs/reference/all-settings#auto-close), [Colorize brackets](https://zed.dev/docs/reference/all-settings#colorize-brackets), `%` in [Vim](https://zed.dev/docs/vim) |
| LSP diagnostics | Have | `lsp.diagnostics` events, underline in severity color with a tooltip (`EditorNativeViewport.attributed`), issue count in the status bar; no next/previous-diagnostic navigation, no gutter or scrollbar marks. | [Diagnostics](https://zed.dev/docs/diagnostics) |
| LSP go to definition, references | Have | `Cmd+click` and context menu → `file.definition` / `file.references` (`soyeht-lsp/src/client.rs`, `EditorPaneView.openDefinition`). | [Go to definition and references](https://zed.dev/docs/configuring-languages#go-to-definition-and-references) |
| LSP hover | Out of scope | Owner decision (2026-10-02): the LLM agents cover it, so do not propose it again. Diagnostic tooltips stay. | [Hover information](https://zed.dev/docs/configuring-languages#hover-information) |
| LSP completion | Out of scope | Owner decision (2026-10-02): the LLM agents cover it, so do not propose it again. Contract 09 §4 already left it out of v1. | [Completions](https://zed.dev/docs/completions) |
| Minimap | Lack | None; out of v1 by contract 09 §4. | [Minimap](https://zed.dev/docs/reference/all-settings#minimap) |
| Git gutter | Have | `file.diff` against HEAD feeds added/modified/deleted gutter bars, per-line blame in the status bar and a side-by-side sheet (`EditorDiffPresentation.swift`, `EditorDiffSheet`). | [Git gutter](https://zed.dev/docs/reference/all-settings#git-gutter), [Git](https://zed.dev/docs/git) |
| Large-file performance | Have | Piece table with line index (`buffer.rs`), bounded viewport paging (`EditorViewportPaging`), incremental tree-sitter, CI gates: open 100k lines < 150 ms, search 1 GB < 1 s, VM key work p95 < 8 ms (`scripts/check_editor_performance.py`). Caveats: each write rebuilds a buffer from the full draft (`Service::prepare_edit`), and files above 50 MiB are refused. | No dedicated large-file page; [Performance](https://zed.dev/docs/performance) covers profiling |
| Keybindings | Partial | Fixed VS Code-like set (`Cmd+P`, `Cmd+Shift+P`, double Shift, `Cmd+D`, `Cmd+/`) plus optional bounded Vim (ADR 0054); no user keymap or base-keymap choice. | [Key bindings](https://zed.dev/docs/key-bindings) |
| File tree / project panel | Have | `EditorProjectPanel` browses the workspace lazily (`project.tree.list`), hides `.gitignore`d entries with a toggle and never shows `.git`, previews on click and keeps on double click or Return, creates, renames and trashes entries confined to the workspace (`project.entry.*`), reveals the active file, moves with the arrow keys, and follows disk changes from FSEvents/inotify file events (`project.tree.watch`, ADR 0074; polling only where events are unavailable). A sidebar button in the editor header and `Cmd+Shift+E` show or hide it, per workspace; when it is kept open in a narrow window, the panes stack or tab instead (ADR 0074). No multi-selection, drag and drop of entries, git status colors or folder auto-fold. | [Project panel](https://zed.dev/docs/project-panel) |

Shared code: every row marked Have or Partial keeps its logic in
`SoyehtClient` or the engine, except the text view and gutter, which are
AppKit; the project panel and tab strip draw shared models
(`EditorProjectTree`, `EditorTabPolicy`). The iPad has no code editor pane yet (`apps/ios/Sources` only hosts
writing surfaces), so the visible-lines renderer is the prerequisite for any
iPad parity.

## Closed by this change

In-file find and replace moves from **Partial** to **Have** (ADR 0073):

- Engine: `soyeht-editor/src/find.rs` counts every match, steps to the next or
  previous one with wrap-around, and builds Replace All as at most 32 folded
  edits; `ContentMatcher` gains whole-word, regex and `$1` expansion. A command
  edit is its own undo transaction (`EditStep::record_command`).
- Protocol: `file.find`, `file.replace`, `FileFindQuery`, `FileFindDirection`,
  `FileFindParams`, `FileFindMatch`, `FileFindResult`, `FileReplaceParams`,
  `FileReplaceResult`.
- Shared client: `EditorFindController` (query, toggles, "3 of 41", next,
  previous, Replace, Replace All), `EngineClient.findInEditorFile` pages to the
  match and selects it, `EditorViewportPaging.landing` clamps every landing.
- Mac: `EditorFindBar` replaces AppKit's find panel; `Cmd+F`, `Cmd+Option+F`,
  `Cmd+G`, `Cmd+Shift+G`, the Edit > Find menu and Escape.

Two fixes ride along:

- After undo or redo the caret lands on the restored edit, clamped to the
  line and the document, and the status bar follows: the caret position was
  written to a SwiftUI binding during a view update, where it could be
  dropped, so `Ln, Col` kept the pre-undo position.
- A click right of a short line puts the caret at its end: AppKit shrank the
  horizontally resizable text view to its longest line, so such clicks hit the
  clip view. `EditorTextLayout.fit` keeps it at least as wide as the visible
  area.

## Project panel and tabs

Closed by the follow-up change (ADR 0072):

- Engine: `editor_tabs` table (schema 29), `editor.tabs.list`, `editor.tabs.move`,
  `file.open {preview}`, save-before-close on `pane.close`, and the
  `project.tree.list|watch` and `project.entry.create|rename|delete` methods in
  `editor/project_*.rs`, with path-escape, symlink, create, rename and delete
  tests.
- Shared client: `EditorProjectTree` (tree, reveal, keyboard),
  `EditorTabPolicy` (activation, cycling, preview, reorder) and the
  `EditorWorkspaceReplica` state behind them, ready for the iPad.
- Mac: `EditorProjectPanel` and `EditorTabBar` replace the open-files list and
  the decorative tabs; the editor destination shows the active tab only.

## Panel toggle, file events and soft wrap

Closed by the change of 2026-10-02 (ADR 0074):

- Project panel: a sidebar button in the editor header, View > Toggle
  Project Panel (`Cmd+Shift+E`), and a choice kept per workspace
  (`EditorProjectPanelPreference`). `PaneLayoutFit` keeps a panel that was
  forced open by stacking or tabbing the panes. It folds the panel only when
  not even a lone editor fits with it.
- Engine: `project.tree.watch` follows folders from `notify` file events
  (FSEvents, inotify). It coalesces each burst, reads only the touched folders
  and keeps the one-second poll only where no watcher starts. The protocol is
  unchanged.
- Soft wrap: `EditorWrapLayout` (shared) and `EditorTextLayout` (shared
  AppKit), View > Soft Wrap (`Cmd+Option+Z`).

## Prioritized plan

Ranked by how soon a person editing code hits the gap, then by cost.

1. **Undo/redo and edit integrity**: done (ADR 0071).
2. **In-file find and replace**: done (ADR 0073).
3. **Project panel**: done (ADR 0072): a lazy worktree tree with open,
   create, rename and delete, plus closable, reorderable tabs.
4. **LSP hover and completion**: out of scope by owner decision
   (2026-10-02). The editor stays simple; the LLM agents cover completion and
   hover. Do not propose them again.
5. **Soft wrap**: done (ADR 0074), in the shared visible-lines layout
   (`EditorWrapLayout`), which the iPad can reuse.
6. **Project panel toggle and file events**: done (ADR 0074).

Then, in order: indentation commands (Tab/Shift-Tab, tab size, outdent on
close brace); per-language highlight queries; bracket-match highlight and jump;
diagnostic navigation and gutter marks; keymap customization; minimap last.
