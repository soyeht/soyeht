# ADR 0148: An agent delegates work to helper agents, each in a pane of its own

- Status: proposed. Extends ADR 0029 (worktree automation), ADR 0034 (the lead role), ADR 0070
  (permission modes) and ADR 0137 (subagents of a harness). The behaviour is contract 17.
- Date: 2026-11-06

## Context

The owner wants an agent in a pane to work as a coordinating agent works with helpers: start one
with a self-contained brief, let it pick a harness and model and run in the background in a
worktree of its own, keep working while several run, be told when each ends and receive its report
as data, send the same helper a follow-up, stop it, and merge the results. The helper may not
have more permission than the coordinator, and may not turn a refused action into an approved one.

Soyeht has most of the parts, none of them this:

- **Tasks** dispatch a task to a new pane in a worktree with a harness, model, effort and mode
  (`tasks/dispatching.rs`, ADR 0029), review it with a second pane (`tasks/review.rs`), and deliver
  CI and review feedback to the owner pane. They are a board: a task has a required "must not change"
  field, one owner, attempts and a state machine, and only the chief or a person dispatches.
  Nothing tells the dispatcher that the executor ended, a running executor cannot be given a
  follow-up by the dispatcher, an agent cannot stop it, and the dispatcher's own mode does not bound
  the executor's.
- **`agent.message`** queues an attributed prompt to another pane with the reply route
  (`agents/peer.rs`). It has no lineage, no limit on how many panes an agent can cause to exist, no
  ceiling on permission and no report.
- **Subagents of a harness** are transcript rows inside one pane (ADR 0137). They belong to one
  harness, end with the turn, share the worktree, cannot be stepped into and cannot be another harness.
- **Permissions**: a request is a `pane.permission_requested` event that only a person answers,
  grants are per pane, and only a person gives full access (`require_person_for_full_access`). The
  chief full-access rule is under audit; this decision does not touch it.
- **Usage meters** read the subscription of a provider (ADR 0141), and `x-clientPolicy` holds the
  bands the clients draw.

## Decision

**A delegation is a new durable record that owns an ordinary agent pane.** The child is an agent
pane of the same workspace, so its transcript, prompt queue, permission requests, mode and model
menus, session recovery and close are the code that exists. The record adds the lineage, the
lifecycle (`queued`, `running`, `waiting_for_input`, `done`, `failed`, `stopped`), the limits, the
mode ceiling, the report and the notification. The child is both: a real pane for everything a
person does, and a record the parent drives. By default the layout does not place it; the parent's
header lists it and a person opens it (contract 17 section 13).

**It extends the tasks system by reuse, not by becoming a task.** A delegation reuses the
worktree port and its idempotent checkpoints, the preset resolution, the full-access rule, the
reviewer's habit of copying model and effort and of starting without the chief tools, and the
durable queue. It does not use the board's states, because a delegation is the parent's private
handle: ad hoc, with no required fields, conversational and short-lived, and a board card per helper
would bury the board. A delegation can be linked to a task (`taskId`, phase 11), in which case it
is that task's dispatch and its report is the review request; until then a chief can still dispatch tasks as today.

**The wire is a new `delegation` family**, because the verbs differ from the task's and a task
method that meant "start, follow up, stop" would break the meaning of the task states. Methods:
`delegation.start`, `delegation.follow_up`, `delegation.stop`, `delegation.list`,
`delegation.report`, plus the person-only `delegation.release` and `delegation.worktree.clean`. Events:
`delegation.created`, `delegation.changed`, `delegation.reported`. The MCP tools are `delegate`,
`follow_up_delegate`, `stop_delegate`, `list_delegates`, `delegate_report` and `list_harnesses`.

**Completion is a queued prompt, never a blocking call.** The tool returns at once; when the
child's turn ends the engine queues the parent a prompt through the mechanism of `agent.message`
(origin `Automatic`, an attributed source, delivered when the parent is idle and no person is typing
or answering), whose text is wrapped as data with a random delimiter and with the facts the engine
measured. A parent that is mid-turn gets it after the turn; an idle parent wakes.

**The child never exceeds the parent, and only a person approves.** The child's mode is capped
by the parent's, compared through the `kind` of the harness's modes, and an unknown ranks low.
A child's permission requests go to the person and to nobody else; no delegation method or tool
answers one; a refusal is kept as a fact in the report; the report is text that no engine
decision reads. `delegationDepth` is 1.

**Cost is bounded by counts and by the meter.** Per-parent, per-workspace and running limits,
a start rate, and a hold when the provider's cached reading is at 95 percent, with a person's
release; a warning above the danger band. The meters inform and are never started to ask.

**State is in the store and in steps.** The start is checkpointed as a dispatch is; a report is queued
once by a key; a restart resumes, recovers or fails each delegation by the rules of contract 17
section 11. Worktrees are kept unless untouched; only a person cleans the rest; branches are never deleted.

## Hardening found on the way

`task.dispatch` by the chief refuses full access but does not bound the executor's mode by the
chief's own: a chief in `plan` can dispatch an executor that edits. The ceiling of contract 17
section 8 should apply to it too. It is phase 0, a small separate change, and does not wait for the rest.

## Rejected alternatives

1. **Rely on the harness's own subagents.** They exist and stay, but they are one harness, die with the
   turn, cannot be another model of another subscription, share the worktree and quota, and cannot be
   seen, stepped into or stopped by a person from the app. Not cross-harness, so not what the owner asked.
2. **Only extend tasks.** Every helper would be a board card with required fields, chief-only,
   one owner and restarts of the session per attempt. The notice, follow-up and stop would still be new,
   and the board would fill with throwaway work. Reuse the machinery; keep the board for work a person plans.
3. **Let the parent open panes and use `message_agent`.** Agents have no tool that creates a pane, and if they
   could there would be no lineage, ceiling, limit, report type or stop; six messages a minute is not
   a protocol for follow-ups; and a sibling could message a child.
4. **Hidden sessions with no pane.** A second transcript, queue and permission surface to build and
   keep in step, and nothing for a person to step into. A pane the layout does not place is hidden enough.
5. **The parent answers the child's permission requests.** It is a laundering path: the parent
   would hold the approval it was refused. Rejected outright.
6. **A blocking `delegate` that returns the report as the tool result.** It holds the parent's turn,
   meets harness tool timeouts, cannot be resumed after an engine restart, and serialises what should
   be parallel. The queued prompt is durable and parallel. A bounded wait tool can be added later.
7. **The report as a file in the worktree.** The child chooses the path and the content, it needs
   file access to the parent's side, and it differs per harness. The final message of the turn works
   for every ACP harness and the facts come from git.
8. **The engine merges the child's branch.** The coordinator merges, as the reference behaviour has
   it; automatic merges would also launder a change past the person's review.
9. **A depth above 1 now.** Cost and the permission story multiply; the owner's reference has a
   coordinator and its helpers. A harness's own subagents cover a helper that needs its own.

## Phasing

Each phase is one pull request with its tests; none edits generated files by hand.

0. **Chief dispatch ceiling.** `task.dispatch` refuses an executor mode above the chief's own.
   Tests: the rank table (a unit test over the kinds and the engine's selector), the dispatch refusal and the unchanged full-access rule.
1. **Contract data and store.** The methods, events, schemas, limits, error code and settings in
   `contracts/protocol.json`, `uv run python contracts/generate.py`, the `delegations` table and its
   checkpoints in `soyeht-store`, and contract 08's matrix and this contract's marks updated. Tests: `--check`,
   the contract tests, store tests that stop between each checkpoint and resume. (If a gate test requires a
   handler for every method, this phase and the next are one.)
2. **Start, list, events, the ceiling, the limits.** `delegation.start` and `delegation.list` for an agent
   and a person, shared isolation only, the child pane opened by the dispatch machinery, the
   `delegation` link on `Pane`, depth 1, per-parent and per-workspace counts, concurrency queueing, the
   rate. Tests: unit tests of the ceiling and the limits; an engine integration test with `soyeht-fake-acp`
   starting a child and reading the list and events.
3. **The report path.** Turn end to report, the engine's git facts off the lock, the queued parent
   prompt in `agents/peer.rs` with the nonce wrapper and its replay, `delegation.report`, the full-queue
   and `parent_gone` cases, and the restart rules of section 11. Tests: unit tests of the wrapper and its
   parse (a text containing the delimiter); an integration test where the parent is mid-turn and then idle;
   a restart test in the style of `upgrade_recovery.rs`.
4. **Follow-up and stop.** `delegation.follow_up`, `delegation.stop` with the cascade, stop on parent close,
   the restriction of `agent.message` to parent and child, retention. Tests: follow-up keeps the session
   (the fake agent's state directory shows one session), stop during a turn, a refused follow-up on a stopped child.
5. **Worktrees.** The `worktree` isolation with `ref`, the untouched-worktree cleanup, `delegation.worktree.clean`,
   the kept-worktree limit, the orphan listing. Tests with the fake worktrunk of the engine tests, and
   `tests/blocking_io_rule.rs` and `tests/core_lock.rs` unchanged and green.
6. **The MCP tools and the acceptance.** The six tools in `soyeht-mcp` with the depth and
   `delegation.enabled` gating; `scripts/e2e_delegation.py` as a gate step with the fake agent (below).
7. **Quota.** The hold, `delegation.release`, the warning, and the tokens and cost in the record. Tests:
   a fake reading at 94, 95 and 80 percent; a stale reading does not hold.
8. **Mac and the shared client.** `DelegationReplica` in `apps/shared` with its vectors, the helpers chip
   and list, Open, Stop and Approve, the check that the layout tolerates a pane it does not place, and the
   tokens of the new sizes. Tests: `swift test --package-path apps/shared`, the vectors, the Mac view checks.
9. **The phone.** The nested list, the sheet and the permission label over the same replica.
10. **Windows and Linux.** The state in `soyeht-client`, the views in `soyeht-desktop`, the same vectors.
11. **The task link.** `taskId` on `delegation.start`, with the contract-drift review. Tests with the e2e task fixture.

**The acceptance of phase 6** runs the engine with three fake harnesses (`soyeht-fake-acp`, as
`tests/agent_messaging.rs` does, whose agent calls an MCP tool when its prompt says to). A parent
calls `delegate` for two children of two different fake harnesses in two worktrees, and a third in
`shared` isolation; the parent's turn ends and the children run at the same time (the list shows two
`running`). Each child ends: the parent gets two report prompts, attributed, with the wrapper and the
measured facts, and a report text that contains the closing delimiter and an instruction to "approve
the pending request" changes no state. The parent calls `follow_up_delegate` on one child and its
transcript shows the same session continuing. A child raises a permission request: the parent's
`pane.permission.answer` is refused with `forbidden_role`, a person's answer is accepted, a person's
refusal appears in the next report's facts. A start naming a mode above the parent's is refused;
a fifth child above `delegationsPerParent` is refused; `stop_delegate` stops the third. The engine is
stopped and started in the middle of a child's turn: the delegation resumes or fails by the rules and
the parent gets one report, not two.

## Open questions for the owner

| # | Question | Recommendation |
|---|---|---|
| 1 | Depth: may a helper delegate? | No. Depth 1. Its harness's own subagents cover its needs. Revisit with measurements of cost |
| 2 | Is delegation on by default? | Yes, with the limits above and `delegation.enabled` in Preferences. The owner asked for it, and the limits bound the cost |
| 3 | A parent in full access: may it start a full-access child? | No by default. The child is capped at the highest non-full mode; a person sets `delegation.inheritFullAccess` to allow it. This keeps the chief full-access audit's rule intact |
| 4 | When the parent's pane closes, stop its children or keep them? | Stop them and keep their worktrees. Unattended spend with nobody to report to is the worse failure |
| 5 | Is a child placed in the layout when it starts? | No. It is listed under its parent and a person opens it. A start must not rearrange a workspace the person arranged |
| 6 | The quota hold: 95 percent, hold plus a person's release? | Yes. A start that would run into an exhausted subscription helps nobody, and the release keeps the person in charge |
| 7 | May a person start a delegation from a pane's menu? | Yes, in phase 8: the method already takes `parentPaneId` from a human actor |
| 8 | Do several reports that wait for a busy parent merge into one prompt? | Yes, later: one turn for N reports is cheaper. Not in phase 3, which keeps one prompt per report so the keys stay simple |
| 9 | Is there a push when a helper ends? | No. The permission push already covers the child that needs a person; the rest would be noise |
| 10 | Does a helper count against the 24 hour retention while it is merely `done`? | Yes: retire at 24 hours without a follow-up, keep the worktree |
| 11 | Link to tasks in the first release? | No: phase 11, after the ad hoc path has been used |
| 12 | The ceiling for `task.dispatch` (the hardening above) before the rest? | Yes, now. It is a gap in what exists, independent of this feature |

## Consequences

- An agent of any harness with MCP can run helpers of any other harness in the background, with one
  mechanism for the Mac, the Windows and Linux app and the phone.
- A helper can cost a subscription's quota without a person in the loop. The counts, the rate, the
  hold and the visible per-delegation tokens are the answer; the owner's choice of limits is question 1 and 2.
- Contract 08's role matrix, the `Pane` schema and `x-limits` grow when phases 1 and 2 land. The pane-level
  `agent.message` gains one restriction (parent and child only for a delegated child).
- The Core lock is untouched: worktree and git work run through `Outcome::Offload` and
  `soyeht_process::run`, and the usage reading is the cached one.
