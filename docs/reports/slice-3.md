# Slice 3 · workspace tasks

Date: 2026-09-12. Profile: `Soyeht.next/dev`. License: Apache-2.0. Tested code
commit: `aded701c01e7b4d7579e48a8b480e098bf3d374c`.

Slice 3 is complete. All sixteen automated acceptances and the local real-agent
procedure passed. The delivery adds durable workspace tasks, atomic claiming,
resumable dispatch, worktrunk isolation, a pane-bound MCP task service, review
and CI feedback, a contract-drift reviewer, typed blocking, an attention inbox,
per-pane loops, a lead role, the task CLI and the Mac board. No tag or release
was created.

## Delivered scope

The versioned protocol adds the complete task object graph, seventeen task and
loop methods, thirteen task and loop events, and six task-specific error kinds.
Rust, Swift and TypeScript types and vectors remain generated from the same
schema. Existing slice 1 and slice 2 messages were unchanged.

The store owns all task SQL. Migrations add tasks, subitems, comments, attempts,
move history, dependencies, blocking, dispatch journals, reviewer checkpoints,
retry deadlines, feedback state, attention and loop configuration. Atomic claim
uses one real SQLite transaction and a guarded version update, so two panes
cannot reserve the same task.

The engine applies the closed state table, records every accepted movement and
returns typed refusals. Task effects use `TaskStorePort`, `WorktreePort` and
scripted revision/CI ports. Dispatch checkpoints worktree creation, pane
creation, binding and prompt delivery so restart resumes after the last durable
effect. A dead executor returns active work only after ptyd reconciliation.

worktrunk creates one branch and worktree per attempt. Cleanup is explicit and
refuses an active process, a dirty tree, an unreachable head or an unknown
condition. The engine never deletes the task branch.

The pane MCP server receives an engine-issued binding outside its arguments.
The engine enforces role, pane, task, generation and attempt again at the API
boundary. Production sessions receive one bound task service; capability probes
receive none. A server that loses the engine socket reconnects once and repeats
the exact request with the same idempotency key.

The executor lifecycle requires both drift declaration lists, closes every
attempt with a typed result and keeps corrections in the same attempt. Typed
blocking handles dependencies, missing information and two bounded external
retries. Human cancellation stops the active turn before closing the attempt.

The drift reviewer checkpoints proposal and findings independently, accepts
either arrival order across restart, rejects stale writes and fails closed on
malformed output. It matches declared contract changes by path, name, label,
updated contract reference and changed path. `incomplete` never passes. A diff
with no recognized public surface must produce an empty finding list.

Feedback uses durable fingerprints and a three-delivery ceiling. Human prompts,
task feedback and loop ticks have stable priority without adding another wire
origin. The attention inbox is derived and published by the engine. Loop rounds
advance only on actual idle-pane dispatch and stop on a limit, explicit off or
three consecutive empty claims.

The CLI covers every task and loop method, including bound executor identity.
The shared Swift client replaces board, attention and loop state from snapshots
and generated events. The Mac app provides the board/list switch, required task
dialog, detail view, engine-owned drag decisions, review badges and attention
navigation. Its generated event switch is exhaustive.

## Acceptance evidence

| Acceptance | Result | Verification |
| --- | --- | --- |
| 1 · create | passed | Dialog validation and the engine reject an empty boundary; valid person and MCP creations publish a `WS-n` task with actor history. |
| 2 · dispatch | passed | A real worktree, one pane, durable binding and complete first prompt are created; an owned task returns `task_has_owner`. |
| 3 · atomic next | passed | Two live panes race in the e2e suite; one guarded transaction wins and only that pane starts an attempt. |
| 4 · loop next | passed | `task_next` answers `willRestartSession: true`, then the same pane opens a new generation in the task worktree. |
| 5 · pane death | passed | A killed real test process returns the task once; a ptyd-live pane and `no_signal` pane do not auto-return. |
| 6 · subitems | passed | Review is refused until all subitems finish; the real executor created and finished two before requesting review. |
| 7 · blocking | passed | Dependency completion/cancellation differ, missing information enters attention, and external failure retries exactly twice. |
| 8 · feedback | passed | Fingerprint deduplication, three-delivery limit, green rearm, draft/permission deferral and SYSTEM'S TURN are covered. |
| 9 · drift | passed | All labels, declarations, changed references, extra edits, malformed output, restart ordering and an undeclared fallback are covered. |
| 10 · moves | passed | The engine table and Mac board accept `in_progress → todo`; `todo → completed` shows `transition_refused`. |
| 11 · retries | passed | A stale attempt is refused and a dispatch resumed after worktree creation creates no second effect. |
| 12 · edits | passed | Version conflicts, self/cross-workspace dependencies and cycles fail with typed errors. |
| 13 · loop | passed | Busy ticks do not accumulate; actual rounds stop after the configured limit, off, or three empty claims. |
| 14 · lead | passed | The lead role receives coordination tools and prompt, while executor-only completion is refused. |
| 15 · attention | passed | All five attention sources publish engine-owned items with stable removal and pane navigation. |
| 16 · cleanup | passed | Process, dirt, reachability and unknown-state refusals are tested; accepted cleanup preserves the branch. |
| 17 · real Mac procedure | passed | The Mac board drove a real executor and reviewer, accepted and refused moves, and a post-restart loop completed a task then stopped after three empty results. |

`scripts/e2e_tasks.py` runs the four slice-specific workflow acceptances under
disposable launchd jobs. It is part of `scripts/check_all.sh` beside the six
shell and ten agent acceptances.

The real procedure used three Dev tasks. `WS-1` completed after two subitems, a
fixture edit, milestone comments and a second reviewer pane. `WS-2` recorded the
accepted and refused board moves before cancellation. `WS-3` was claimed by a
loop after an engine restart, completed with two subitems and an approved
review, then left the same pane idle after three distinct empty claims. The
full reproducible record is [the slice 3 manual procedure](../manual-acceptance-slice-3.md).

## Local merge gate

Hosted GitHub Actions remains disabled by project decision. Every PR used the
local gate with the private restricted vocabulary, exact base commit and final
PR payload. The last implementation run, for commit
`a77cba09d708ef8802b2366ddca4ff87338023c6`, passed:

- 257 repository test declarations, up from 256 at its base;
- 24 automated acceptances: 6 shell, 10 agent and 4 task workflows;
- all 26 gate steps, with no removed test, acceptance or step;
- generated protocol/profile files with no diff;
- Rust formatting, clippy with warnings as errors, build and workspace tests;
- 477 typed Swift vectors and 3 binary vectors;
- 13 shared Swift client tests and unsigned Mac/iPhone builds;
- 484 TypeScript protocol tests;
- zero SQL outside the store and zero production panic sites; and
- zero production functions above 120 lines, with the maximum still 120.

The restricted-vocabulary scan passed over files, commit messages and final PR
text. The report PR repeats the same gate from the final implementation base.

## Run from scratch on a clean Mac

1. Install the pinned tools from [toolchains](../toolchains.md). Configure the
   local development-signing account and the official ACP adapter without
   storing credentials in the repository.
2. Clone the repository, create a dedicated worktree and run
   `scripts/check_all.sh` with `FORBIDDEN_TERMS` supplied through the
   environment.
3. Build the Rust workspace and install only the new Dev profile with
   `python3 scripts/install_dev.py`. Confirm the engine reports schema version
   10 and a connected ptyd.
4. If macOS presents a Documents-folder request for `soyeht-ptyd`, allow it
   before diagnosing an ACP session as stalled. The task worktree inherits the
   repository's parent directory.
5. Build and open the development-signed Mac app. Register the official adapter
   in the Dev harness registry as described in [the agent CLI guide](../agent-cli.md).
6. Follow [the slice 3 manual procedure](../manual-acceptance-slice-3.md) in a
   disposable Git repository. Record no credential, binding or pairing payload.

## Decisions and limits

- ADR 0029 adopts worktrunk through a bounded JSON port and preserves branches.
- ADR 0030 records two durable stages for drift review.
- ADR 0031 defines reviewer execution and recovery in the live engine.
- ADR 0032 defines durable bounded retry timing for external failures.
- ADR 0033 records task executor defaults and override precedence.
- ADR 0034 defines durable pane loops and the lead role boundary.
- ADR 0035 derives feedback priority from stable prompt identities without a
  new wire origin.

macOS may gate Documents access for the launchd supervisor on first use. The
manual procedure makes that system consent explicit. Denial is visible as a
waiting native session and requires a person to change the operating-system
decision.

Reviewer output is intentionally strict. Bare JSON and one JSON code fence are
accepted; extra prose or an unknown surface fails closed and returns the task.
The model may therefore require a correction turn rather than receive an
inferred approval.

A loop depends on its agent calling `task_next`. The engine still owns idleness,
reservation, restart, attempt identity and stop counters, so repeated or stale
agent calls cannot create a second executor.

Task branches and refused worktree cleanup remain for a person to inspect.
Automatic branch deletion, cross-workspace boards, recurring event triggers,
sub-subitems, the PR contract and the iPhone task board remain outside this
slice.

## Delivery PRs

1. [Protocol](https://github.com/soyeht/soyeht/pull/69)
2. [Task persistence](https://github.com/soyeht/soyeht/pull/70)
3. [Task rules and store port](https://github.com/soyeht/soyeht/pull/71)
4. [Resumable worktree dispatch](https://github.com/soyeht/soyeht/pull/72)
5. [Scripted feedback decisions](https://github.com/soyeht/soyeht/pull/73)
6. [Deterministic ACP task workflow](https://github.com/soyeht/soyeht/pull/74)
7. [Pane-bound MCP task service](https://github.com/soyeht/soyeht/pull/75)
8. [Durable session binding](https://github.com/soyeht/soyeht/pull/76)
9. [Task authoring API](https://github.com/soyeht/soyeht/pull/77)
10. [Executor lifecycle](https://github.com/soyeht/soyeht/pull/78)
11. [Pane-stop return](https://github.com/soyeht/soyeht/pull/79)
12. [Human task moves](https://github.com/soyeht/soyeht/pull/80)
13. [Drift policy](https://github.com/soyeht/soyeht/pull/81)
14. [Review checkpoints](https://github.com/soyeht/soyeht/pull/82)
15. [Live drift reviewer](https://github.com/soyeht/soyeht/pull/83)
16. [Typed blocking](https://github.com/soyeht/soyeht/pull/84)
17. [Dispatch and task next](https://github.com/soyeht/soyeht/pull/85)
18. [Pane loops and lead role](https://github.com/soyeht/soyeht/pull/86)
19. [Feedback and attention](https://github.com/soyeht/soyeht/pull/87)
20. [Safe worktree cleanup](https://github.com/soyeht/soyeht/pull/88)
21. [Task CLI](https://github.com/soyeht/soyeht/pull/89)
22. [Launchd task acceptances](https://github.com/soyeht/soyeht/pull/90)
23. [Shared task replica](https://github.com/soyeht/soyeht/pull/91)
24. [Mac task screens](https://github.com/soyeht/soyeht/pull/92)
25. [Create-and-dispatch recovery](https://github.com/soyeht/soyeht/pull/93)
26. [Empty drift review handling](https://github.com/soyeht/soyeht/pull/94)
27. [MCP reconnection after engine restart](https://github.com/soyeht/soyeht/pull/95)

The PR containing this report records the final gate and closes slice 3.
