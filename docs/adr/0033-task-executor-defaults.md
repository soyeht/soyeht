# ADR 0033: Task executor defaults

Status: accepted for slice 3 on 2026-09-12.

## Context

The task contract allows an executor configuration to select the workspace
default. The current workspace model has no stored default harness. Dispatch
still needs one concrete harness before it can open an agent pane.

## Decision

An explicit dispatch resolves a workspace-default executor to the first entry
in the ordered harness registry. The task stores that resolved harness and the
workspace-default flag becomes false. A task pulled with `task.next` inherits
the calling pane's harness, model, effort and permission unless the task
overrides an optional value.

This keeps every attempt reproducible from its durable task and attempt
records. A future workspace-default setting must replace this rule through a
schema and migration change.

## Consequences

Dispatch fails with `harness_unavailable` when the registry is empty. Registry
order is part of profile configuration for this slice. Pulling work never
changes the pane identifier or its MCP binding, but it opens a fresh native
session with a new pane generation in the task worktree.
