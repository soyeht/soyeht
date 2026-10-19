# ADR 0094: A file that cannot be text is a state of the editor

- Status: accepted
- Date: 2026-10-18

## Context

Opening a binary file in the editor answered `invalid_params` with the text
"file is not UTF-8", and the Mac showed it in the same "Could not complete" dialog
that every failed action uses. The pane the person was looking at stayed as it was, and
the dialog blocked it. The desktop client put the same text in its problem banner. A
client could not tell this answer from any other refused path, so it could not do better.

## Decision

The engine answers `file.open` with two new error kinds instead of `invalid_params`:
`file_not_text` (the bytes are not UTF-8 text) and `file_too_large` (the file exceeds
the editor's size limit). Any other refusal of the read, such as a path that is not a regular
file, stays `invalid_params`.

Where a failure is shown follows what failed:

- A file the editor cannot show is a state of the editor. The pane says why in place of the
  text (`x-clientPolicy` texts `editorFileNotText` and `editorFileTooLarge`), the tab strip and
  the project panel stay usable, and nothing needs confirming. The state lasts until another file
  opens or a tab is chosen.
- Every other failure of an action stays the problem dialog or banner: an edit the engine
  refused (the person may have lost keystrokes), a rename or a delete, terminal input, a lost
  connection, a path that was refused, a file that is gone.

The rule lives in `EditorOpenRefusal` (`apps/shared`) and in `soyeht_client::editor::refusal`
on the desktop; both pass the `openFailure` cases of `contracts/vectors/editor.json`. A client
that opens a file calls `reportEditorOpenFailure` (Swift) or sends the `Open` command (desktop)
and never sets the problem itself for a refusal.

## Consequences

A client built before this change shows the engine's own text in its dialog, since the
message is unchanged apart from its wording. A new kind of failure that should be a state of
a surface adds a case to the vector first.
