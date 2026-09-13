# ADR 0054: Keep Vim input pure and bound Ex effects

Status: accepted · 2026-09-13

## Context

Vim mode is optional and must not replace the editor's normal bindings when it is
off. Normal, insert and visual input needs deterministic cursor and edit behavior.
The four allowed Ex commands cross different boundaries: save, close and reload
use existing file or pane operations, while substitution may cover a file larger
than the visible viewport.

## Decision

The shared client owns a pure Vim state machine. It receives a key, immutable
viewport text and selections, then returns selections, a typed edit plan or one of
four closed effects. AppKit only translates keys and applies that result. The
preference is off by default, persists per client, and bypasses the state machine
entirely when disabled.

`:w` drains queued edits and waits for the engine-owned save. `:q` closes the pane
through the engine. `:e` discards no durable draft and reloads the engine-owned
buffer. `:%s` reads bounded pages at one pinned revision, builds literal UTF-16
edits, and submits one revision-checked write. The command fails before mutation
if it exceeds 4,096 edits or the protocol edit-byte budget.

## Consequences

Input-mode tests need no view, socket or process. An unsupported Ex command has no
effect, and shell commands cannot be introduced through the command line. A whole
file substitution is atomic and cannot partially apply, even when the file spans
multiple viewports. Large substitutions fail explicitly instead of falling back
to a partial edit.
