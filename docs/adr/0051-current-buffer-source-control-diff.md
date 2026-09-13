# ADR 0051: Current-buffer source-control diff

- Status: accepted
- Date: 2026-09-13

## Context

The editor can hold a durable draft that is newer than the working-tree file.
The `file.diff` contract identifies the open file and its editor revision, but
does not define a separate baseline selector, line-number origin, or partial
response cursor.

## Decision

`file.diff` compares the current editor buffer, including its unsaved draft,
against the file at `HEAD`. An untracked file uses an empty baseline. Diff and
blame line numbers are zero-based, matching the editor protocol. Blame is
reported only for unchanged current-buffer lines because changed lines do not
have a truthful author in `HEAD`.

Every request checks the editor revision and re-resolves the stored relative
path through the editor's canonical workspace boundary before invoking the
source-control port. A path that has become an escaping symlink is rejected.

The operation returns a complete bounded result. If the diff cannot fit within
the JSON frame budget, it fails without returning a partial hunk because the
current protocol has no diff cursor.

## Consequences

The gutter and side-by-side view describe exactly what the user is editing,
including a draft that autosave has not written yet. A stale view cannot attach
diff or blame facts to a newer revision. Large diffs remain explicit failures
until a future protocol adds pagination.
