# 0001 — Monorepo and incremental delivery of slice 1

Status: accepted for implementation, 2026-09-10.

## Context

The objective requires one PR per step and blocking checks from the first
PR. Implementing the six process acceptance tests depends on ptyd and the engine,
which follow the protocol. The private repository does not support branch
protection on the current plan. Execution authorization permits squash merges after green CI.

## Decision

The protocol, Rust crates, new Swift applications, design and documentation live
in the same repository. Existing prose was moved without content changes
to `docs/contracts`. Internal analysis stays outside the repository. The source
of executable formats lives in `contracts/protocol.json`. Project license:
Apache-2.0, following Caio's explicit correction on 2026-09-10.

Each step delivers code usable at its level and real checks:
the protocol has serialization and framing tests in all three languages;
ptyd has process tests; the engine introduces the six complete acceptances.
No nonexistent job is represented by a test that always passes.
Application jobs build their targets from their introduction in the
corresponding step. Until those steps, a PR does not claim to deliver the entire slice.

Main receives no direct pushes. Every change uses its own worktree and branch,
with a PR and squash merge after green CI, as authorized by Caio. Subsequent
review may produce further PRs. If CI is unavailable, the same checks run
locally, with evidence in the PR and an issue to fix the runner.
We do not claim mechanical branch protection on GitHub.

## Rationale

Steps must be reviewable without simulating components that have not yet
been implemented. The final report may only declare delivery with all final
gates active and passing. This order does not change the acceptances or waive
any of them.
