# ADR 0035: Task feedback scheduling

Status: accepted for slice 3 on 2026-09-12.

## Context

The task contract requires review feedback to enter the durable pane queue
after human prompts and before loop prompts. It also requires one delivery per
failure fingerprint, a limit of three deliveries per task, and no accumulated
polling work while the engine is stopped. It does not specify the status poll
interval or how the two existing wire-level prompt origins represent three
priority classes.

## Decision

The engine polls review tasks at most once every 30 seconds while it is
running. Poll deadlines stay in memory, so an engine restart performs one
fresh inspection without replaying missed polls. The fingerprint, delivery
counter, and rearm flag remain durable in the task store.

Human prompts retain the `human` origin. Review feedback and loop prompts
retain the existing `automatic` origin and use stable prompt identity prefixes
to recover their class: `task-feedback:` precedes `loop:`. Queue restoration
sorts by human, feedback, then loop while preserving first-in order inside each
class. The prompt rows, task update, feedback checkpoint, and attention change
commit together before publication.

A refused human card move creates one stable attention item when the task has
an owning pane. Repeating the same refusal does not duplicate the item. The
next accepted move removes it in the same transaction as the move. An
unowned-card refusal is returned directly because every attention item in the
current protocol must lead to a real pane.

## Consequences

Restart cannot reverse feedback and loop priority. Review polling cannot
produce a backlog, and repeated identical failures or move refusals stay
bounded. The protocol gains no new prompt-origin value during slice 3.
