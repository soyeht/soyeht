# ADR 0039: New-task verification field

- Status: accepted
- Date: 2026-09-12

## Context

The approved new-task dialog does not show the existing optional how-to-verify field. The Mac
client already binds and sends that value. This alignment objective permits presentation changes
only, so removing the input would change behavior.

## Decision

The description field's paperclip affordance opens a small editor for optional verification notes.
The value continues through the existing task creation request without protocol or client changes.
The dialog's default appearance preserves the approved panel structure.
