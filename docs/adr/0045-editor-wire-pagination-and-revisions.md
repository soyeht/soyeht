# ADR 0045 · Editor wire pagination and revisions

Date: 2026-09-12
Status: accepted for slice 4

## Context

The editor contract names file, language, review, and conflict operations but does not define their payloads. A whole-file payload can exceed the protocol's 1 MiB JSON limit and contradict viewport-only rendering. Text positions also need one coordinate system shared by Swift, the editor core, and language servers.

## Decision

Wire positions are zero-based line and UTF-16 column pairs. This matches the native text system and the default language-server position encoding. The editor core converts at its boundary and rejects a position that splits a scalar value.

`file.open` returns file metadata and the first bounded viewport. `file.read` is the only content reader and returns at most `editorViewportLines` lines from the buffer, disk, draft, or one history version. No method or event carries an unbounded whole file.

Each open file has an engine-owned decimal revision. Every edit, format, revert, and review mutation names the base revision. A stale base is refused. Multi-edit requests are one ordered transaction; ranges must not overlap. Each mutation also carries the existing idempotency key.

A disk conflict publishes the base, draft, and disk revision identities. The client reads either side through bounded `file.read` calls. This provides both versions without making an event exceed the transport limit.

Project search and optional replacement share `file.search`. The registry marks the method as a mutation because replacement can write. A search-only call still carries an idempotency key and is safe to repeat.

History is paged and capped by the generated limits. Diff and review results use stable hunk identifiers tied to a file revision. Editor files are included in snapshots so reconnecting clients replace their local replicas before subscribing to later events.

The protocol version remains 1 because no released client consumes this repository. Adding the `editor` pane mode and the editor methods is part of the first unreleased contract.

## Consequences

Clients can render and reconcile large files without buffering an unbounded response. Revisions close write, disk-change, and hunk-review races. The engine must validate UTF-16 boundaries, ordered non-overlapping edits, source-specific history identifiers, and every generated size limit before invoking an external port.
