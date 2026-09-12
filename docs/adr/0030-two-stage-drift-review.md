# ADR 0030: Two-stage contract-drift review

Status: accepted for slice 3 on 2026-09-12.

## Context

The task contract requires the drift reviewer to start when a task enters
`in_review`. The executor's `contractChanges` and `outOfScopeEdits`
declarations arrive later in the required `task.complete` request. Approval
depends on both facts, and neither may be inferred or discarded after an
engine restart.

## Decision

Review has two durable inputs. `task.request_review` moves the task to
`in_review`, opens the reviewer pane, and starts classification of every
touched public function, handler, and protocol message. `task.complete`
records the executor's summary and both required declaration lists as a
completion proposal. It does not complete the task by itself.

The engine resolves the proposal only after it has one reviewer label for
every touched surface. A missing, duplicate, or unknown surface is treated as
`incomplete`. Contract labels pass only when the matching declaration cites a
contract or ADR changed in the same revision. An `extra_edits` label passes
only with a matching nonempty reason. `incomplete` never passes.

An approved result closes the attempt and moves the task to `completed`. A
returned result keeps the attempt open, moves the task to `in_progress`, and
adds the per-surface result as a task comment. Both paths are idempotent and
publish their state only after the review result and completion proposal are
durable.

## Consequences

The reviewer may finish before or after the executor submits its completion
proposal. Restarting the engine can resume either order without approving on
partial evidence or asking the executor to repeat a successful request.
