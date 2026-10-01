# Editor gap analysis against Zed

Date: 2026-10-01. Scope: the everyday editing features of the code editor pane
(engine `engine/soyeht-editor`, `engine/soyeht-engine/src/editor`,
`engine/soyeht-engine/src/languages`; Mac UI `apps/mac/Sources/Editor*`; shared
client `apps/shared/Sources/SoyehtClient/Editor*`), compared with Zed's public
documentation at <https://zed.dev/docs>. Contract 09 (`docs/contracts/09-editor.md`)
is the closed v1 feature list; its section 4 puts completion, minimap and
folding out of v1.

Marks: **Have**: works end to end. **Partial**: exists with a material
limitation. **Lack**: absent. The table reflects `main` before this change. The
last row group says what this change closes.

## Feature table

| Feature | Mark | Our evidence | Zed reference |
| --- | --- | --- | --- |
| File open, save, dirty state | Have | `file.open` (`editor/api.rs`); every accepted write persists a draft and autosaves 500 ms later (`editor/persistence.rs`, ADR 0047); header shows `· unsaved` / `· saved` / `· conflict` (`EditorPaneView.swift` `EditorPaneHeader`); disk changes reload or raise a conflict. No explicit `Cmd+S`: it is swallowed (`EditorTextView.performKeyEquivalent`). | [Autosave](https://zed.dev/docs/reference/all-settings#autosave) |
| Multiple tabs / buffers | Partial | `EditorTabs` lists every open file of the workspace and selects its pane; the `xmark` is decoration only, so a tab cannot be closed from the strip, and there is no recent-order switcher. | [Editor tabs](https://zed.dev/docs/reference/all-settings#editor-tabs), [Tab Switcher](https://zed.dev/docs/tab-switcher) |
| Undo / redo | Lack | Edits bypass AppKit's undo registration (`Coordinator.apply` writes the text storage directly), so `Cmd+Z` rewound nothing the engine knew; no engine undo method existed. | `editor::Undo` in [Vim: restoring common keybindings](https://zed.dev/docs/vim#restoring-common-text-editing-and-zed-keybindings); rename preview "you can undo the changes" in [Configuring languages](https://zed.dev/docs/configuring-languages#rename-symbol) |
| Paste, cut, drag and drop, IME | Lack | Paste, cut and drop go through `NSTextView.shouldChangeText`, never through the custom `insertText` path, so they changed the page without an engine edit (reproduced with a headless `NSTextView`); dead-key composition left the marked character behind. | Baseline text-editing behavior; see [Multibuffers](https://zed.dev/docs/multibuffers#editing-in-a-multibuffer) for paste across cursors |
| Multi-cursor | Partial | `Cmd+D` next occurrence and `Opt+click` add cursor (`EditorNativeViewport.swift`, `EditorEditing.nextOccurrence`); search covers only the loaded page; no add-cursor above/below, no select-all-occurrences. | [Multibuffers](https://zed.dev/docs/multibuffers#editing-in-a-multibuffer), [Multi cursor modifier](https://zed.dev/docs/reference/all-settings#multi-cursor-modifier) |
| Find and replace (in file) | Partial | Only AppKit's find panel (`usesFindPanel = true`), which sees the loaded page (at most `editorViewportLines`), not the file; replace through it bypassed the engine. Project-wide search and replace exist (`file.search`, palette `.project`, `EditorNavigationPalette.replaceAll`). | [Search settings](https://zed.dev/docs/reference/all-settings#search), `buffer_search::Deploy` in [Vim](https://zed.dev/docs/vim#restoring-common-text-editing-and-zed-keybindings), [Project search](https://zed.dev/docs/finding-navigating#project-search) |
| Go to line | Have | Palette mode `.line` (`EditorNavigationPalette.goToLine`) loads a page at the line; status bar shows `Ln, Col`. | `:<number>` in [Vim ex commands](https://zed.dev/docs/vim#ex-commands), [Line indicator format](https://zed.dev/docs/reference/all-settings#line-indicator-format) |
| Syntax highlighting | Partial | Incremental tree-sitter for Rust, Swift, TypeScript, Python, JSON, Markdown, shell (`soyeht-editor/src/syntax.rs`), viewport-only projection. Classification is a node-kind heuristic with a short shared keyword list, not per-language highlight queries, so many tokens stay uncolored; no semantic tokens; 7 languages. | [Syntax highlighting and themes](https://zed.dev/docs/configuring-languages#syntax-highlighting-and-themes), [Semantic tokens](https://zed.dev/docs/semantic-tokens) |
| Soft wrap | Lack | The text container never tracks the view width (`widthTracksTextView = false`, horizontally resizable). | [Soft wrap](https://zed.dev/docs/reference/all-settings#soft-wrap) |
| Line numbers and gutter | Have | `EditorLineGutter` draws numbers, the current line, git markers, agent review and agent cursors; no diagnostic or fold markers, no relative numbers. | [Gutter](https://zed.dev/docs/reference/all-settings#gutter), [Relative line numbers](https://zed.dev/docs/reference/all-settings#relative-line-numbers) |
| Indentation, auto-indent | Partial | Newline keeps the indent and adds four example after `{[(` (`EditorEditing.insertionPlan`); no Tab/Shift-Tab block indent, no tab size or hard-tab setting, no outdent on `}`, no indent guides. | [Auto indent](https://zed.dev/docs/reference/all-settings#auto-indent), [Tab size](https://zed.dev/docs/reference/all-settings#tab-size), [Hard tabs](https://zed.dev/docs/reference/all-settings#hard-tabs), [Indent guides](https://zed.dev/docs/reference/all-settings#indent-guides) |
| Bracket matching | Partial | Auto-close and type-over of `()[]{}""''` (`EditorEditing.closingCharacter`); no matching-bracket highlight, no jump to the match. | [Auto close](https://zed.dev/docs/reference/all-settings#auto-close), [Colorize brackets](https://zed.dev/docs/reference/all-settings#colorize-brackets), `%` in [Vim](https://zed.dev/docs/vim) |
| LSP diagnostics | Have | `lsp.diagnostics` events, underline in severity color with a tooltip (`EditorNativeViewport.attributed`), issue count in the status bar; no next/previous-diagnostic navigation, no gutter or scrollbar marks. | [Diagnostics](https://zed.dev/docs/diagnostics) |
| LSP go to definition, references | Have | `Cmd+click` and context menu → `file.definition` / `file.references` (`soyeht-lsp/src/client.rs`, `EditorPaneView.openDefinition`). | [Go to definition and references](https://zed.dev/docs/configuring-languages#go-to-definition-and-references) |
| LSP hover | Lack | No `textDocument/hover` request in `soyeht-lsp`; only diagnostic tooltips. | [Hover information](https://zed.dev/docs/configuring-languages#hover-information) |
| LSP completion | Lack | No `textDocument/completion`; contract 09 §4 marks completion out of v1. | [Completions](https://zed.dev/docs/completions) |
| Minimap | Lack | None; out of v1 by contract 09 §4. | [Minimap](https://zed.dev/docs/reference/all-settings#minimap) |
| Git gutter | Have | `file.diff` against HEAD feeds added/modified/deleted gutter bars, per-line blame in the status bar and a side-by-side sheet (`EditorDiffPresentation.swift`, `EditorDiffSheet`). | [Git gutter](https://zed.dev/docs/reference/all-settings#git-gutter), [Git](https://zed.dev/docs/git) |
| Large-file performance | Have | Piece table with line index (`buffer.rs`), bounded viewport paging (`EditorViewportPaging`), incremental tree-sitter, CI gates: open 100k lines < 150 ms, search 1 GB < 1 s, VM key work p95 < 8 ms (`scripts/check_editor_performance.py`). Caveats: each write rebuilds a buffer from the full draft (`Service::prepare_edit`), and files above 50 MiB are refused. | No dedicated large-file page; [Performance](https://zed.dev/docs/performance) covers profiling |
| Keybindings | Partial | Fixed VS Code-like set (`Cmd+P`, `Cmd+Shift+P`, double Shift, `Cmd+D`, `Cmd+/`) plus optional bounded Vim (ADR 0054); no user keymap or base-keymap choice. | [Key bindings](https://zed.dev/docs/key-bindings) |
| File tree / project panel | Partial | `EditorFileTree` lists only files already open; it cannot browse the worktree or create, rename or delete files. `Cmd+P` opens any file by fuzzy name (`file.search` path scope). | [Project panel](https://zed.dev/docs/project-panel) |

Shared code: every row marked Have or Partial keeps its logic in
`SoyehtClient` or the engine, except the text view, gutter and file tree, which
are AppKit. The iPad has no code editor pane yet (`apps/ios/Sources` only hosts
writing surfaces), so the visible-lines renderer is the prerequisite for any
iPad parity.

## Closed by this change

Undo/redo and the paste/cut/drop/IME path move from **Lack** to **Have**:

- Engine: `soyeht-editor` `EditHistory` records each accepted buffer change as
  forward and inverse UTF-16 edits, groups writes closer than 300 ms, bounds the
  history to 200 transactions and 16 MiB, and drops it when the file changes
  through a path it did not record. `file.undo` / `file.redo` replay through the
  durable write path (ADR 0071).
- Protocol: `file.undo`, `file.redo`, `FileUndoParams`, `FileUndoResult`.
- Shared client: `EngineClient.replayEditorHistory`, `EditorEditing.verbatimPlan`,
  `EditorViewportPaging.landingStart`, Vim `u`.
- Mac: `EditorTextView` routes every AppKit text change into an engine edit or
  refuses it, keeps IME marked text local until commit, and sends `Cmd+Z`,
  `Cmd+Shift+Z` and Edit-menu undo/redo to the engine.

## Prioritized plan

Ranked by how soon a person editing code hits the gap, then by cost.

1. **Undo/redo and edit integrity**: done here. It was a correctness bug: the
   first paste or `Cmd+Z` silently diverged the screen from the file.
2. **In-file find and replace**: a `Cmd+F` bar backed by the engine's buffer
   search (`soyeht-editor/src/search.rs` already scans whole text) with match
   count, next/previous, case and regex toggles, and replace through
   `file.write`. Drop AppKit's find panel, which sees one page.
3. **Project panel**: a real worktree tree (the engine already enumerates
   project files with ignore rules in `editor/project_files.rs`) with open,
   create, rename and delete, plus closable tabs.
4. **LSP hover and completion**: add `textDocument/hover` and
   `textDocument/completion` to `soyeht-lsp`, a popover in the shared client.
   Completion needs the owner to lift contract 09 §4.
5. **Soft wrap**: belongs in the shared visible-lines renderer (needed for the
   iPad anyway); wrapping inside the current `NSTextView` page would be thrown
   away.

Then, in order: indentation commands (Tab/Shift-Tab, tab size, outdent on
close brace); per-language highlight queries; bracket-match highlight and jump;
diagnostic navigation and gutter marks; keymap customization; minimap last.
