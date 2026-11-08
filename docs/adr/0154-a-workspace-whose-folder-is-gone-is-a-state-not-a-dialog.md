# ADR 0154: A workspace whose folder is gone is a state, not a dialog

- Status: accepted
- Date: 2026-11-08

## Context

A workspace is a folder. When the folder is deleted or moved while the workspace stays (a
scratch folder under a temporary directory, a disk that was ejected), every request that reads
it failed with `internal_error` and the text "I/O operation failed". The Mac listed the project
tree when the workspace was shown, so the failure came up as the "Could not complete" dialog at
every launch, with nothing the person could do about it but press OK.

## Decision

The engine answers a request that cannot find the workspace's folder with a new error kind,
`workspace_folder_missing` (code -32042). One rule makes it: opening the workspace root maps a
missing folder to this kind (`errors::workspace_root`), and the project tree and the editor's
path resolution, the project watch, the project search and replace, and the writing section
listing all open the root through it. The message is "The workspace folder is missing"
and names no path, since a message can reach a log; the client knows the path from the workspace.
A path that is missing inside a folder that is there stays `not_found`.

The clients treat it as a state of the workspace and not as the failure of an action:

- `EngineClient` marks the workspace when a project listing, an open, a diff or a reload of its
  files ends in this kind, sets no `problem`, and clears the mark when the root lists again or
  the workspace closes (`missingWorkspaceFolder(in:)`).
- The Mac shows a bar under the top bar, and the iPad's board shows one above the panes, with
  the text "This workspace's folder is gone: <path>" (`x-clientPolicy` text
  `workspaceFolderGone`) and "Close workspace", which asks as any close does.
- The engine has no method to point a workspace at another folder, so closing is the only
  choice offered. A method to relocate it would add a second button to the same state.

The desktop client does not open a dialog for a folder it cannot list: it collapses the folder.
It gets the typed kind and the clearer text in the editor pane, and no bar.

## Consequences

A client built before this change shows the engine's message in its dialog, now saying that the
folder is missing. A new request that opens the workspace root maps its failure with
`errors::workspace_root`, so the kind stays one rule.

Not covered: the working directory of a new terminal pane (`panes.rs`) still answers an I/O
error when the folder is gone, since it resolves a directory the caller named and not the
workspace root. A later change can map it with the same function.
