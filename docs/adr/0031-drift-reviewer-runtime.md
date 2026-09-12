# ADR 0031: Drift reviewer runtime boundary

Status: accepted for slice 3 on 2026-09-12.

## Context

The task contract requires a second agent pane to review the candidate diff.
It defines the review result and its ordering, but it does not choose the
reviewer's harness configuration or grant that pane task mutation tools.
Those choices affect isolation, restart behavior, and reproducibility.

## Decision

The reviewer uses the executor's harness, model, and effort. It always uses
the `ask` permission mode. Its ACP session receives the candidate patch in the
review prompt and receives no task MCP server or task binding.

The engine stores the reviewer pane, prompt, revision snapshot, executor pane
identity, completion proposal, findings, and decision. It resolves a review
only when the task still belongs to that executor and attempt. An old review
is retired without changing the task.

Malformed reviewer output fails closed as `incomplete`. An approved review
closes the attempt. A returned review keeps the same attempt and queues the
review comment for the executor. A later review cycle uses a new reviewer pane
and a new version of the same attempt's review record.

## Consequences

Review can resume after an engine restart with either durable input arriving
first. The reviewer can inspect the supplied patch but cannot mutate the task
through its session. Changing the executor's harness configuration also
changes the reviewer configuration for the next review cycle.
