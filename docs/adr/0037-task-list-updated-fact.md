# ADR 0037: Task list updated fact

- Status: accepted
- Date: 2026-09-12

## Context

The approved task-list frame includes an `UPDATED` column. The current task protocol publishes
`createdAt`, but it does not publish `updatedAt`. Deriving a timestamp in the client would make the
client the owner of a task fact and could show a value that disagrees with the engine.

## Decision

The Mac task list preserves the approved column and displays an em dash until the engine publishes
an update timestamp. The client does not derive or substitute another timestamp.

Adding the fact requires a later protocol change under a functional objective. This design-only
alignment does not change the protocol or task behavior.
