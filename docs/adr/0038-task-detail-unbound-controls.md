# ADR 0038: Task detail unbound controls

- Status: accepted
- Date: 2026-09-12

## Context

The approved task-detail frame includes a comment field and a send-for-review action. The current
Mac client has no bindings for either operation. This objective permits presentation changes only;
adding client methods or engine behavior is outside its boundary.

## Decision

The detail panel includes these elements as noninteractive presentation previews. They are hidden
from accessibility so they are not announced as available controls. Existing task data, close, open,
move, and dispatch behavior remains unchanged.

A later functional objective must either bind the two elements to existing protocol methods or
remove them from the approved design. This pass does not add a protocol or client operation.
