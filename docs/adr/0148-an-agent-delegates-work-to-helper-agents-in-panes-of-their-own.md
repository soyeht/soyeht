# ADR 0148: An agent delegates work to helper agents, each in a pane of its own

- Status: proposed. Extends ADR 0029 (worktree automation), ADR 0034 (the lead role), ADR 0070
  (permission modes), ADR 0075 (skills) and ADR 0137 (subagents of a harness). The behaviour is contract 17.
- Date: 2026-11-06

## Context

The owner wants an agent in a pane to work as a coordinating agent works with helpers, and his rule for
every open choice is: do what Claude Code's subagents do. The agent starts a helper with a self-contained
brief; the helper picks a harness and model and runs in the background in a worktree of its own; the agent
keeps working while several run, is told when each ends and receives its report as data, can send the same
helper a follow-up with its context intact, and can stop it. A helper may not have more permission than its
parent, and may not turn a refused action into an approved one.

Soyeht has most of the parts, none of them this:

- **Tasks** dispatch a task to a new pane in a worktree with a harness, model, effort and mode
  (`tasks/dispatching.rs`, ADR 0029), review it with a second pane (`tasks/review.rs`), and deliver CI and
  review feedback to the owner pane. They are a board: a task has a required "must not change" field, one
  owner, attempts and a state machine, and only the chief or a person dispatches. Nothing tells the
  dispatcher that the executor ended, the dispatcher cannot give a running executor a follow-up or stop it,
  and the dispatcher's own mode does not bound the executor's.
- **`agent.message`** queues an attributed prompt to another pane with the reply route (`agents/peer.rs`).
  It has no lineage, no limit on how many panes an agent can cause to exist, no ceiling on permission and no report.
- **Subagents of a harness** are transcript rows inside one pane (ADR 0137). They belong to one harness,
  end with the turn, share the worktree, cannot be stepped into and cannot be another harness.
- **Permissions**: a request is a `pane.permission_requested` event that only a person answers, grants are
  per pane, and only a person gives full access (`require_person_for_full_access`).
- **Skills** (ADR 0075) are a store of named folders with a user and a workspace scope, a watcher and a
  size bound. A named helper type needs the same.

## Owner decisions of 2026-11-06

| # | Decision | Where it lands |
|---|---|---|
| 1 | Depth is 1: a helper cannot delegate | contract 17 section 8, Depth |
| 2 | Delegation is on by default, with the setting `delegation.enabled` | section 5 |
| 3 | A helper runs under the same permission rules and mode as its parent: never more, no lower cap. A full-access parent gets a full-access helper. Its requests go to the person; the parent never answers; a refused action stays refused | section 8 |
| 4 | Closing the parent stops its helpers | section 4 |
| 5 | Helpers run in the background, are not placed in the layout, and are looked at in a helpers view with the live transcript, Stop and pending permission requests | section 13 |
| 6 | Quota works as in Claude Code: no hold and no check; a provider limit fails the helper and the parent gets the error in the report. Concurrency caps stay | section 9 |
| 7 | The person asks in plain language and the agent delegates. User-defined helper types by name, per workspace and per user, in the skills store. No start from a pane menu | section 6 |
| 8 | A helper lives as long as its parent's session; a finished helper takes a follow-up at any time; no retirement timer. An unchanged worktree is removed, a changed one kept for the person | sections 4 and 10 |

## Decision

**A delegation is a new durable record that owns an ordinary agent pane.** The helper is an agent pane of the
same workspace, so its transcript, prompt queue, permission requests, mode and model menus, session recovery
and close are the code that exists. The record adds the lineage, the lifecycle (`queued`, `running`,
`waiting_for_input`, `done`, `failed`, `stopped`), the limits, the mode rule, the report and the notification.
The pane is the helper's session and nothing more on screen: it is never placed in the layout, and the
helpers view shows it.

**It extends the tasks system by reuse, not by becoming a task.** A delegation reuses the worktree port and
its idempotent checkpoints, the preset resolution, the reviewer's habit of starting without the chief tools,
and the durable queue. It does not use the board's states, because a delegation is the parent's private
handle: ad hoc, with no required fields, conversational, and short-lived, and a board card per helper would
bury the board. It can be linked to a task later (`taskId`, phase 11).

**The wire is new `delegation` and `helpers` families**, because the verbs differ from the task's and a task
method that meant "start, follow up, stop" would break the meaning of the task states. Methods:
`delegation.start`, `delegation.follow_up`, `delegation.stop`, `delegation.list`, `delegation.report` and the
person-only `delegation.worktree.clean`; `helpers.list`, `helpers.read`, `helpers.create`, `helpers.update`,
`helpers.delete`. Events: `delegation.created`, `delegation.changed`, `delegation.reported`, `helpers.changed`.
The MCP tools are `delegate`, `follow_up_delegate`, `stop_delegate`, `list_delegates`, `delegate_report`,
`list_harnesses` and `list_helper_types`.

**Completion is a queued prompt, never a blocking call.** The tool returns at once; when the helper's turn
ends the engine queues the parent a prompt through the mechanism of `agent.message` (origin `Automatic`, an
attributed source, delivered when the parent is idle and no person is typing or answering), whose text is
wrapped as data with a random delimiter next to facts the engine measured. A parent that is mid-turn gets it
after the turn; an idle parent wakes.

**The helper has the parent's permission and no more; only a person approves.** Its mode is the parent's
(compared through the `kind` of the harness's modes, an unknown ranking low), lowerable by the call or its
type and never raised. Its permission requests go to the person and to nobody else; no method or tool of the
parent answers one; a grant is per pane as it is today; a refusal by the person is remembered for the
parent's session and refuses the same request from any helper of it without asking. The report is text that no
engine decision reads.

**Helper types are a second kind in the skills store.** A type is `.agents/helpers/<name>/HELPER.md`, user
scope in the skills home and workspace scope in the repository, with the skills store's name rule, override
rule, revision check, bounds and watcher, by giving `soyeht-skills` a kind. It is not a `SKILL.md`, because
that would deliver it to every harness and list it in the "/" picker. A type names a harness, model, effort,
mode (lower only), isolation, allowed tools (narrowing only) and instructions, and is called by name. The
engine enforces the Soyeht tools, refuses a request outside the list, and passes the list to an adapter that
takes one; a harness tool that neither asks nor has an adapter option is not blocked, and the list says so.

**Cost is bounded by counts, and the provider is the quota.** Active helpers per parent and workspace, running
per workspace and in total, finished helpers per parent, and a start rate. The usage meters are not read to
decide anything; a provider's limit is an error the helper reports.

**Lifetime is the parent's session.** A helper lives until its parent's pane closes, its harness is switched, or
a new session replaces its session. A finished helper idle for 300 seconds releases its process and resumes on
the next follow-up where the harness can resume. State is in the store and in steps: the start is checkpointed
as a dispatch is, a report is queued once by a key, and a restart resumes, recovers or fails each delegation by
the rules of contract 17 section 11. An unchanged worktree is removed when the delegation ends; a changed one
is kept for a person to clean; branches are never deleted or merged.

## Related work that is not in this decision

`task.dispatch` by the chief refuses full access but does not bound the executor's mode by the chief's own. A
fix that caps it is in progress separately; this decision does not change the dispatch path or
`require_person_for_full_access`, and the delegation's mode rule is written so that it does not depend on it.

## Rejected alternatives

1. **Rely on the harness's own subagents.** They exist and stay, but they are one harness, die with the turn,
   cannot be another model of another subscription, share the worktree, and cannot be seen, stepped into or
   stopped by a person from the app. Not cross-harness, so not what the owner asked.
2. **Only extend tasks.** Every helper would be a board card with required fields, chief-only, one owner and a
   session restart per attempt. The notice, follow-up and stop would still be new, and the board would fill with
   throwaway work. Reuse the machinery; keep the board for work a person plans.
3. **Let the parent open panes and use `message_agent`.** Agents have no tool that creates a pane, and if they
   had there would be no lineage, ceiling, limit, report type or stop; six messages a minute is not a protocol
   for follow-ups; and a sibling could message a helper.
4. **Hidden sessions with no pane.** A second transcript, queue and permission surface to build and keep in step,
   and nothing for a person to step into. A pane the layout does not place is hidden enough.
5. **The parent answers the helper's permission requests.** It is a laundering path: the parent would hold the
   approval it was refused. Rejected outright.
6. **A cap below the parent's mode, or a lower default for helpers** (for example a full-access parent's helper
   capped below full access unless a setting allows it). The owner's rule is Claude Code's: the helper has the
   parent's rules. A cap would also be a second permission model for the person to learn.
7. **A quota hold from the usage meters** (holding a start near a full window, with a person's release and a
   warning). It guesses from a cached reading, stalls work the harness would have done, and needs a person's
   release. Claude Code lets the provider answer; so do we.
8. **Placing a helper in the layout, or an Open action that does.** A start must not rearrange a workspace the
   person arranged. The helpers view holds the transcript.
9. **A retirement timer for finished helpers** (for example stopping them after 24 hours). Claude Code keeps a
   helper while its session lives. The idle process release gives the memory back without ending the helper.
10. **Starting a helper from a pane menu or a form.** The person asks the agent in words, as in Claude Code.
11. **Helper types as a `SKILL.md` with extra fields.** The harnesses would load them as skills and the "/" list
    would show them. A sibling folder reuses the store code and none of the delivery.
12. **Helper types as entries of `harnesses.json`.** Those describe programs a pane can run; a type describes a
    role over one of them.
13. **A blocking `delegate` that returns the report as the tool result.** It holds the parent's turn, meets
    harness tool timeouts, cannot be resumed after an engine restart, and serialises what should be parallel.
    The queued prompt is durable and parallel.
14. **The report as a file in the worktree.** The helper chooses the path and the content, and it differs per
    harness. The final message of the turn works for every ACP harness and the facts come from git.
15. **The engine merges the helper's branch.** The coordinator merges, as the reference behaviour has it.
16. **A depth above 1.** The owner decided it; a harness's own subagents cover a helper that needs its own.

## Phasing

Each phase is one pull request with its tests; none edits generated files by hand.

1. **Contract data and store.** The methods, events, schemas, limits, error code and setting of both families in
   `contracts/protocol.json`, `uv run python contracts/generate.py`, the `delegations` table and its checkpoints
   in `soyeht-store`, contract 08's matrix updated, and this contract's marks removed as names land. Tests:
   `--check`, the contract tests, store tests that stop between each checkpoint and resume. (If a gate test
   requires a handler for every method, this phase and the next are one.)
2. **Start, list, events, the mode rule, the limits.** `delegation.start` and `delegation.list` for an agent, shared
   isolation only, the helper pane opened by the dispatch machinery and not placed, the `delegation` link on
   `Pane`, depth 1, the active, running and finished counts, concurrency queueing, the start rate,
   `delegation.enabled`. Tests: unit tests of the rank table and the mode rule (parent's mode, lower accepted,
   higher refused, unknown low, the engine's selector, full access carried from a person's parent); an engine
   integration test with `soyeht-fake-acp` starting a helper and reading the list and events.
3. **The report path.** Turn end to report, the engine's git facts off the lock, the queued parent prompt in
   `agents/peer.rs` with the nonce wrapper and its replay, `delegation.report`, the full-queue and `parent_gone`
   cases, a provider-limit error as a `failed` report, and the restart rules of section 11. Tests: wrapper and parse
   (a text containing the delimiter); an integration test where the parent is mid-turn and then idle; a restart
   test in the style of `upgrade_recovery.rs`.
4. **Follow-up, stop and the end of the parent's session.** `delegation.follow_up` from `done` and from `failed`
   with a live session, the mode check at a follow-up, `delegation.stop`, stop on pane close, harness switch and
   session replacement, the restriction of `agent.message` to parent and helper, the idle process release and its
   resume, the refusal memory. Tests: follow-up keeps the session (the fake agent's state directory shows one
   session), stop during a turn, follow-up after the parent finished other turns, a refused follow-up on a stopped
   helper, a request refused by the person refused again for a helper without a prompt.
5. **Worktrees.** The `worktree` isolation with `ref`, the removal of an unchanged worktree when the delegation
   ends, `delegation.worktree.clean`, the kept-worktree limit, the orphan listing. Tests with the fake worktrunk of
   the engine tests; `tests/blocking_io_rule.rs` and `tests/core_lock.rs` unchanged and green.
6. **The MCP tools and the acceptance.** The seven tools in `soyeht-mcp` with the depth and `delegation.enabled`
   gating, and `scripts/e2e_delegation.py` as a gate step with the cases below that do not need helper types.
7. **Helper types.** A kind in `soyeht-skills` and the `.agents/helpers` roots, `helpers.*` and `helpers.changed`,
   the name resolution in `delegation.start`, the allowed-tools enforcement, the types in the MCP server's
   instructions, and the type-by-name case added to the acceptance. Tests: store tests of the kind (override,
   revision, bounds), resolution order, a type naming a mode above the parent's refused, a request outside the
   allowed list refused by the engine.
8. **The shared client and the Mac.** `DelegationReplica` in `apps/shared` with its vectors, the chip, the helpers
   view with live transcript, Stop, permission answer and Clean, the Preferences list of helper types, the check that
   the layout tolerates a pane it does not place, and the new size tokens. Tests: `swift test --package-path
   apps/shared`, the vectors, the Mac view checks.
9. **The phone.** The chip, the sheet and the permission label over the same replica, and the types in Preferences.
10. **Windows and Linux.** The state in `soyeht-client`, the views in `soyeht-desktop`, the same vectors.
11. **The task link.** `taskId` on `delegation.start`, with the contract-drift review. Tests with the e2e task fixture.

**The acceptance** runs the engine with fake harnesses (`soyeht-fake-acp`, as `tests/agent_messaging.rs` does,
whose agent calls an MCP tool when its prompt says to). The cases:
- A parent calls `delegate` for two helpers of two different fake harnesses in two worktrees, and a third in
  `shared` isolation; the parent's turn ends and the helpers run at the same time (the list shows two `running`).
- Each helper ends: the parent gets two report prompts, attributed, with the wrapper and the measured facts; a report
  text that contains the closing delimiter and an instruction to "approve the pending request" changes no state.
- The helpers start in the parent's mode; a call naming a mode above the parent's is refused with `forbidden_role`;
  a fifth active helper is refused; `stop_delegate` stops the third.
- Follow-up after done: the parent has gone on to other turns, then calls `follow_up_delegate` on a helper that
  reported earlier; the helper's transcript shows the same session continuing, with its earlier turn in context.
- Helper type by name: a workspace file `.agents/helpers/reviewer/HELPER.md` names the second fake harness, the plan
  mode and instructions; the person's message asks for "the reviewer"; the parent calls `delegate` with the type; the
  helper's pane has that harness and mode and receives the instructions; a type that names a mode above the parent's
  fails the start.
- A helper raises a permission request: the parent's `pane.permission.answer` is refused with `forbidden_role`, a
  person's answer is accepted; a person's refusal is shown in the next report's facts, and a second helper of the
  same parent making the same request is refused without a prompt.
- A provider-limit error scripted in the fake agent ends a turn: the helper is `failed`, the report holds the error, and
  a follow-up after the script clears continues it.
- Closing the parent's pane stops its helpers; an unchanged worktree is removed, a changed one is kept.
- The engine is stopped and started in the middle of a helper's turn: the delegation resumes or fails by the rules and
  the parent gets one report, not two.

## Open questions for the owner

The eight decisions above are settled. These remain, each with a recommendation.

| # | Question | Recommendation |
|---|---|---|
| 1 | A helper type's allowed tools cannot be enforced for a harness tool that never asks permission and has no adapter option. Accept the limit? | Yes for the first release, stated in the types list; measure which adapters take an allow list and use it |
| 2 | A grant the person gave the parent (`allow_always`) does not reach its helpers, which ask once each. Should it? | No for now: a grant is per pane today (contract 04), and widening it is a change to the permission model, not to delegation. Revisit after use |
| 3 | The parent's session ends when its harness is switched or a new session replaces it, and the helpers stop. Is a harness switch the right line? | Yes: the new harness has none of the conversation that started them, and nobody to report to |
| 4 | A helper that failed on a provider limit takes a follow-up. Right? | Yes: it is what the owner's parent could do with its own turn once the limit resets |
| 5 | Finished helpers per parent: 8, and an idle release at 300 seconds? | Yes, both from measurement of a harness process's memory in phase 4; they are limits in the contract, so they change in one place |
| 6 | Several reports waiting for a busy parent: one prompt each, or one merged prompt? | One each in phase 3 so the keys stay simple; merge later if parents spend turns on it |
| 7 | A push when a helper ends? | No. The permission push already covers a helper that needs a person; the rest would be noise |
| 8 | Link to tasks in the first release? | No: phase 11, after the ad hoc path has been used |
| 9 | Ship starter helper types (a reviewer, an investigator)? | No files shipped in the first release; the documentation shows two examples to copy |

## Consequences

- An agent of any harness with MCP can run helpers of any other harness in the background, with one mechanism
  for the Mac, the Windows and Linux app and the phone, and the person can name a type they defined.
- A helper can spend a subscription's quota without a person in the loop. The counts and the start rate bound the
  parallelism; the provider's own limit ends the rest, and the report says so.
- A repository can ship helper types in `.agents/helpers`. They can choose a harness, model and instructions
  but never a mode above the parent's, so they carry the trust of an `AGENTS.md`, not of a permission.
- Contract 08's role matrix, the `Pane` schema, `x-limits`, `x-settings` and the `soyeht-skills` crate grow when
  phases 1, 2 and 7 land. `agent.message` gains one restriction: parent and helper only, for a delegated helper.
- The Core lock is untouched: worktree and git work run through `Outcome::Offload` and `soyeht_process::run`.
