# ADR 0072: Project panel and engine-owned editor tabs

- Status: accepted
- Date: 2026-10-01

## Context

The editor's file tree listed only files that were already open, and its tab
strip was decoration: the `xmark` did nothing, tabs could not be reordered, and
each open file appeared as one more split in the editor destination. Zed's
project panel browses the whole worktree lazily, respects `.gitignore`,
creates, renames and deletes entries, and opens files into closable, reorderable
tabs with a single preview slot. The iPad must reuse the same rules, and a
paired device has the same powers as the person at the Mac.

ADR 0046 gives each file exactly one owner pane and makes the engine the only
component that touches the workspace. ADR 0047 promises that an acknowledged
edit is never lost.

## Decision

A tab is a live editor pane; the one-pane-per-file ownership stays. The engine
keeps each workspace's tab order and its single preview tab in `editor_tabs`
(schema 29) and publishes `editor.tabs_updated` whenever `file.open`,
`pane.close`, `editor.tabs.move`, `project.entry.rename` or
`project.entry.delete` changes them. `editor.tabs.list` returns the current
order. Every grid, the agents grid and the editor destination alike, shows the
editor panes as one tabbed area: layout code receives only the active tab's
pane (`EngineClient.layoutPanes`), and a stored layout that names another
editor pane, including a layout saved with one split per file before tabs,
gives that editor slot to the active pane (`PaneLayoutTree.adoptingLiveEditor`).
Switching tabs therefore never moves agent or terminal panes, and nothing is
persisted. What a client stores in `workspace.layout.set` follows the same rule:
the editor area is one leaf, the tab on show, so the engine accepts a layout that
lists one editor pane (or all of them, as a layout saved before tabs does) and
rejects one that lists none while an editor is open; every other open pane still
needs its own leaf. The editor's full minimum width includes the 212-point project
panel, so the pane frame is never narrower than its content. When the panes do
not fit the window at their full floors, `PaneLayoutFit` lays them out at the
editor's compact floor, without the panel, and the editor hides the panel until
its pane is wide enough again; if they still do not fit, it presents them in
one row or column, then puts agent and terminal panes behind one tab strip, so
no pane is placed outside the window. The stored layout is not changed.

`file.open` accepts `preview`. A preview takes the slot of the workspace's
previous preview when that file is unedited and has no draft; the replaced pane
closes in the same mutation. A preview records the file revision it was
previewed at and stays a preview only while the file keeps that revision, so
the first edit pins it without `file.write` touching tabs; the keystroke path is
unchanged. Opening the file again without `preview` (double click, Return,
`Cmd+P`, links, go to definition) pins it.

`pane.close` on an editor pane first runs the pending autosave. A draft that
conflicts with the disk is not written; it stays in the database, and reopening
the file restores it with its conflict and resumes the autosave. The client
drains queued keystrokes before it asks to close, and asks the person only for a
conflicted draft. Closing activates the previously active tab, else the right
neighbour, as Zed's default `activate_on_close: "history"`.

`project.tree.list` lists one folder: folders first, case-insensitive names,
`.gitignore`, `.ignore` and global excludes applied with parent rules (the
`ignore` crate, as project search already uses), ignored entries flagged and
shown only on request, `.git`, `.hg`, `.svn` and `.DS_Store` never shown, at
most 5,000 entries. `project.tree.watch` names the folders a connection shows;
the engine follows them and sends that connection the non-durable
`project.tree_changed`. (It polled them once a second; ADR 0074 replaced the
polling with operating-system file events.)

`project.entry.create`, `project.entry.rename` and `project.entry.delete` act
on workspace-relative paths of plain names. Each folder the path passes through
must canonicalize inside the canonical workspace root, so `..`, absolute paths
and symbolic links that leave the workspace are refused; the last component is
never followed, so deleting or renaming a link acts on the link. Rename never
replaces an existing entry (`renamex_np` with `RENAME_EXCL` on macOS,
`renameat2` with `RENAME_NOREPLACE` on Linux) and moves open buffers, drafts and
local history with the file in one transaction; a closed history row that held
the destination path belongs to a file no longer on disk and is dropped. Delete
saves pending drafts first, refuses while a draft conflicts, moves the entry to
the system trash on macOS (`NSFileManager.trashItemAtURL`), and closes the tabs
of what it removed. Where no trash is available it changes nothing and answers
`needs_confirmation`; the client asks and repeats with `permanent: true`. A file
open as a writing document is refused, since its sidecar moves with the writing
surface.

## Consequences

The tree, tabs and preview behave the same on every paired client; activation
history is per client. Following directories costs one recursive file-event
stream per watched workspace (ADR 0074), bounded at 256 folders per
connection. A rename or delete can race
another process between the confinement check and the system call; the
operation then fails or acts inside the folder that was checked. The trash path
is macOS only for now.
