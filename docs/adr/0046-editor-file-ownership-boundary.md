# ADR 0046: Editor file ownership boundary

- Status: accepted
- Date: 2026-09-12

## Context

The editor contract makes `file.open` the only file operation that accepts a
path. It does not define a separate maximum open-file size, the initial
viewport height, or how `pane.create` names the file to open.

Parsing an unbounded file could hold the engine worker indefinitely. Keeping a
path after `file.open` would also let later operations repeat workspace and
symbolic-link validation inconsistently.

## Decision

The engine canonicalizes both the workspace and requested file at
`file.open`. It rejects absolute paths, parent components, directories,
non-UTF-8 files, and any canonical target outside the canonical workspace.
The durable identity is the canonical workspace-relative path, and every
later operation accepts only the opaque `fileId`.

An open file is limited to the generated 50 MiB editor-history byte budget. A
single version cannot consume more than the complete history budget, and the
limit prevents an unbounded initial parse without introducing a second size
constant. The opening response projects at most 200 lines beginning at the
requested line. Later viewports use the generated 2,000-line protocol limit.

`pane.create` with `mode: "editor"` requires the additional `path` field and
uses the same ownership path as `file.open`; its smaller response remains
`{paneId}`. A scripting client can use this request when it needs the smaller
pane-only response.

## Consequences

Aliases through symbolic links inside a workspace converge on one relative
identity. A link that resolves outside the workspace fails before persistence.
Opening an already-owned file returns its existing editor pane instead of
creating a second owner.

The 50 MiB cap may be lowered only by changing the generated protocol limit
and its tests. Parser progress cancellation remains an independent defense to
add with the measured editor performance gate.
