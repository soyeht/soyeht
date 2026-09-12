# ADR 0034: Loop clock and lead role

Status: accepted for slice 3 on 2026-09-12.

## Context

The task contract calls the loop interval `interval`, while the client
contract describes the same value as seconds. The task contract also promises
that a person can appoint a lead, but the released schema did not yet expose
the promised role mutation.

## Decision

`LoopConfig.interval` is measured in seconds. A missed deadline authorizes one
prompt after the pane becomes idle; missed ticks are never replayed. The round
limit remains a safety limit when it is nonzero. The three-empty stop applies
when `stopWhen` is `three_empty`, and disabling a loop is the explicit-off
stop.

`pane.role.set` is the single role mutation. Its public values are `worker`
and `chief`, matching the existing authenticated engine roles. A person may
change the role only while the pane is idle and owns no task. Entering the
chief role queues the contract's lead instructions once; repeating the same
assignment does not queue another copy. The role is durable and visible in
the pane snapshot. The engine restarts the native agent session after a role
change so its bound MCP server observes the new authorization. Session loading
preserves the harness session when the harness supports it. The protocol
version remains 1 because no client has been released.

## Consequences

Loop state appears in the `loops` snapshot section and changes in the same
transaction as loop prompts and task-pull counters. A busy pane receives at
most one prompt when it becomes eligible, even after a long engine stop.
Changing a role during active task ownership returns `pane_busy`.
