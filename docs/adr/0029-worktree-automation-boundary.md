# ADR 0029: Worktree automation boundary

Status: accepted for slice 3 on 2026-09-11.

## Context

Task dispatch needs an idempotent worktree operation that can resume after the
engine stops between an external effect and its durable checkpoint. The task
contract also requires manual cleanup, safety refusals, and branch retention.

## Decision

The engine uses the worktrunk CLI as a process port. It reads structured output
from `list --format=json --branches`, reuses an existing task branch or
worktree, and creates a missing one with `switch --create --no-cd
--format=json`. Project hooks are disabled for engine-owned operations so a
repository hook cannot turn an internal retry into a second external effect.

Cleanup is available only to a person: the local human client or a paired
device, which has the same powers since the 2026-09-26 owner decision. The engine first reads the
structured listing and refuses when cleanliness is unknown, the worktree is
dirty, its HEAD is not reachable from another branch, its recorded path does
not match, or a process has a current directory inside it. It then uses
`remove --no-delete-branch --foreground --format=json` and verifies that the
structured result names the same branch and path and reports that the branch
was not changed. A refusal is returned to the client with its bounded reason.
The engine never deletes a task branch.

Dispatch records `reserved`, `worktree_created`, `pane_opened`, and
`prompt_accepted`. Worktree preparation, pane opening, and first-prompt
delivery use stable identities, so replaying the first unrecorded step has the
same result rather than creating a second worktree, pane, or prompt.

## Consequences

The engine does not parse human-readable tables or implement Git worktree
mechanics. A missing executable, invalid structured response, or unsafe path
fails the dispatch visibly. Automated tests use a process fixture and port
doubles; they do not require the command to be installed on the runner.
