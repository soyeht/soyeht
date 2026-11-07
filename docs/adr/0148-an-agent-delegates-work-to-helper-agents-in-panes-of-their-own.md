# ADR 0148: An agent delegates work to helper agents, each in a pane of its own

- Status: proposed. Extends ADR 0029 (worktree automation), ADR 0034 (the lead role), ADR 0070
  (permission modes), ADR 0075 (skills) and contract 04 section 5 (subagents of a harness). The behaviour is contract 17.
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
- **Subagents of a harness** are transcript rows inside one pane (contract 04 section 5, `agents/subagents.rs`). They belong to one harness,
  end with the turn, share the worktree, cannot be stepped into and cannot be another harness.
- **Permissions**: a request is a `pane.permission_requested` event that only a person answers, `allow_always`
  grants are per pane, and only a person gives full access (`require_person_for_full_access`).
- **Skills** (ADR 0075) are a store of named folders with a user and a workspace scope, a watcher and a
  size bound. A named helper type needs the same.

## Owner decisions of 2026-11-06

| # | Decision | Where it lands |
|---|---|---|
| 1 | Depth is 1: a helper cannot delegate | contract 17 section 8, Depth |
| 2 | Delegation is on by default, with the setting `delegation.enabled` (the engine ships it false until the clients of phase 8, then true) | section 5 |
| 3 | A helper runs under the same permission rules and mode as its parent: never more, no lower cap. A full-access parent gets a full-access helper. Its requests go to the person; the parent never answers; a refused action stays refused (`reject_always`). A grant (`allow_always`) the person gave the parent covers its helpers, as in Claude Code; `allow_once` stays per request; a grant given on a helper's request covers that helper only; the later decision wins | section 8 |
| 4 | Closing the parent stops its helpers | section 4 |
| 5 | Helpers run in the background, are not placed in the layout, and are looked at in a helpers view with the live transcript, Stop and pending permission requests | section 13 |
| 6 | Quota works as in Claude Code: no hold and no check; a provider limit fails the helper and the parent gets the error in the report. Concurrency caps stay | section 9 |
| 7 | The person asks in plain language and the agent delegates. User-defined helper types by name, per workspace and per user, in the skills store. No start from a pane menu | section 6 |
| 8 | A helper lives as long as its parent's session; a finished helper takes a follow-up at any time; no retirement timer. An unchanged worktree is removed, a changed one kept for the person | sections 4 and 10 |
| 9 | Allowed tools of a helper type are enforced as far as Soyeht can, and the types list says what is not blocked | section 6 |
| 10 | The parent's session ends on pane close, harness switch or a replacement session; the helpers stop | section 4 |
| 11 | A helper that failed on a provider limit takes a follow-up once the limit resets | section 4 |
| 12 | At most 8 finished helpers per parent; an idle finished helper releases its process after 300 s; both confirmed by a memory measurement in phase 4 | section 9 |
| 13 | One report prompt per report for now | section 8 |
| 14 | No push when a helper ends | section 13 |
| 15 | No task link in the first release (phase 11) | section 12 |
| 16 | No starter helper types shipped; the documentation shows two examples | section 6 |

## Decision

**A delegation is a new durable record that owns an ordinary agent pane.** The helper is an agent pane of the
same workspace, so its transcript, prompt queue, permission requests, mode and model menus, session recovery
and close are the code that exists. The record adds the lineage, the lifecycle (`queued`, `running`,
`waiting_for_input`, `done`, `failed`, `stopped`), the limits, the mode rule, the report and the notification.
The pane is never placed in the layout; the helpers view shows it.

**It extends the tasks system by reuse, not by becoming a task.** A delegation reuses the worktree port and its
checkpoints (the steps are extracted into an owner-generic step that tasks and delegations share), the preset
resolution, the reviewer's way of starting a pane with no MCP server when a helper type asks for it, and the
durable queue. It does not use the board's states: a delegation is the parent's private handle, ad hoc, with no
required fields, and a board card per helper would bury the board. It can be linked to a task later (phase 11).

**The wire is new `delegation` and `helpers` families**, because a task method that meant "start, follow up,
stop" would break the meaning of the task states. Methods: `delegation.start`, `delegation.follow_up`,
`delegation.stop`, `delegation.list`, `delegation.report`, the person-only `delegation.worktree.clean`;
`helpers.list`, `helpers.read`, and the person-only `helpers.create`, `helpers.update`, `helpers.delete`,
`helpers.trust`. Events: `delegation.created`, `delegation.changed`, `delegation.reported`, `helpers.changed`.
MCP tools: `delegate`, `follow_up_delegate`, `stop_delegate`, `list_delegates`, `delegate_report`,
`list_harnesses`, `list_helper_types`.

**Completion is a queued prompt, never a blocking call, and it is enqueued exactly once.** A turn's end writes a
`report_due` marker in the transaction that ends the turn and holds the helper's next prompt; the report is made
off the lock; the apply step revalidates the parent and its session and enqueues the prompt under a key. The text is
wrapped as data with a random delimiter, and everything a helper or its repository wrote (name, paths, message) is
inside the delimiters. A follow-up reaches the helper in a second form with no reply route.

**The helper has the parent's permission and no more; only a person approves.**
- Its mode is the parent's, compared through a rank table in the contract data, not `kind` (which is wrong for
  Codex `read-only` and the engine's `accept_edits`). A parent's unknown mode ranks lowest and a helper's unknown
  mode ranks highest. The identifier is never trusted across harnesses.
- The mode the harness **confirms** is checked before the first prompt and at every follow-up, because the initial
  configuration is best effort: a helper above its parent is failed with no prompt sent.
- Its requests go to the person; no method or tool of the parent answers one.
- A grant (`allow_always`) the person gave the parent covers its helpers, through a harness-neutral request
  fingerprint; a grant on a helper covers that helper only; the later decision wins. A `reject_always` is remembered
  for the parent's session and refuses the same request from any helper of it without asking. Both live in a store
  table with a session and a sequence, not in the pane's `permission_rules`. This differs on purpose from the rule
  for a harness's in-pane subagents: it is the owner's request for Claude Code's behaviour.
- The guarantees hold for a connection with an agent binding; the local socket without one is `human_local`, which
  delegation does not change.

**A helper is not its parent's peer, and its allowlist is the engine's.** A helper is `Role::Agent` with a binding
signal. It may call the web and writing methods and `agent.list` (its parent only). The task family (`task.next` would
apply a task's preset mode and restart the session), `loop.set`, `pane.role.set`, `delegation.*`, `helpers.*`,
`harness.list` and `agent.message` in both directions are refused by the engine, not only left out of the tool list. It
reaches its parent only through its report.

**Helper types are a second kind in the skills store.** A type is `.agents/helpers/<name>/HELPER.md`, user scope in the
skills home and workspace scope in the repository, with the skills store's name rule, revision check, bounds and
watcher, by giving `soyeht-skills` a kind. It is not a `SKILL.md`, which would deliver it to every harness and list it
in the "/" picker. A type names a harness, model, effort, a mode (lower only), isolation, whether the helper gets the
Soyeht MCP server at all, allowed tools (narrowing only) and instructions. Because a repository can ship one: the
user's type wins over a repository type of the same name, the MCP server's static instructions name user types only,
the body and the composed prompt are bounded before any side effect, and a repository type is inert until a person
approves its revision.

**Cost is bounded by counts, and the provider is the quota.** Active helpers per parent and workspace, running per
workspace and in total, finished helpers per parent, and a start rate. The usage meters are not read. A limit the
harness reports as an error ends the turn `failed` with the error in the report; one reported as text plus `end_turn`
is an ordinary `done` turn, and there is no classifier without evidence from the real harnesses.

**Lifetime is the parent's session.** A helper lives until its parent's pane closes, its harness is switched, or a new
session replaces its session (`session/new` fallback, a `task_next` restart); a reload keeps it. A finished helper idle
for 300 seconds releases its process and resumes on the next follow-up where the harness can resume. State is in the
store and in steps, and a restart resumes, recovers or fails each delegation by contract 17 section 11. An unchanged
worktree the delegation made is removed when it ends, with its zero-commit `delegate/` branch: an amendment of ADR 0029
and contract 03 R22, limited to those worktrees, retried after the asynchronous close and never automatic on Windows.
A changed one is kept for a person to clean. Nothing is merged.

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
16. **Keeping the person's grants per pane, so each helper asks again** (the rule for the parent's own pane
    today). Claude Code applies a permission the person grants for the session to its subagents; the owner's rule
    is the same, with `allow_once` and refusals handled as written in contract 17 section 8.
17. **A depth above 1.** The owner decided it; a harness's own subagents cover a helper that needs its own.

## Phasing

Each phase is one pull request with its tests. New code goes in new modules (`engine/soyeht-engine/src/delegation/`,
`engine/soyeht-store/src/delegations.rs`); the files this touches (`tasks/dispatching.rs`, `tasks/review.rs`,
`tasks/worktree.rs`, several of `agents/`) are near the 800-line cap and do not grow.

1. **Contract data and store.** The methods, events, schemas, limits, error code and setting of both families, the
   `x-modeRanks` table, the `x-paneNonRouteIds` entries, `uv run python contracts/generate.py`, the `delegations` table
   with its checkpoints, contract 08's matrix. No `taskId`. Tests: `--check`, the contract tests, store tests that stop
   between each checkpoint and resume, and the rank-table coverage test over the pinned registry snapshot (a harness
   with a mode and no row fails). If a gate test needs a handler for every method, this phase and the next are one.
2. **Start, list, events, the mode rule, the helper allowlist and the side channels.** `delegation.start` and
   `delegation.list`; shared isolation only; the pane opened by the dispatch machinery and not placed; the `delegation`
   link; depth 1; the counts and the start rate; the duplicate guard; the composed-prompt bound; the confirmed-mode
   check; the binding signal and the engine allowlist in `dispatch.rs`; `agent.message` refused to and from a helper;
   `agent.list` restricted; the route-less follow-up form in `peer.rs`. `delegation.enabled` ships false. Tests: the
   rank rules (parent's mode, lower accepted, higher refused, unknown in both directions, the engine selector, Codex
   `read-only`, no rank-0 mode under a plan parent); a fake harness that refuses `session/set_mode`, so the helper is
   failed with `mode_exceeds_parent` and no prompt; a helper calling `task.next`, `loop.set` and `pane.role.set`
   refused; an integration test with `soyeht-fake-acp`. The fake agent gains generic commands "call MCP tool X with JSON"
   and "say text" (`soyeht-acp/tests/fake-agent/mcp.rs`).
3. **The report path.** `report_due`, the `reporting` hold, the off-lock report, the revalidating apply step, the
   queued parent prompt with the nonce wrapper and its replay, `delegation.report`, the retry driver for a full queue,
   the outcome mapping, the restart rules. `ReadOnlyGit` gains `rev-list --count`, `diff --numstat` and
   `status --porcelain`. Tests: the wrapper and its parse as a unit test with an injected nonce generator, including a
   field containing the nonce; facts from a real `git worktree` (the fake worktrunk produces none); a parent mid-turn
   then idle; queue full; a stop by a person waking the parent and a stop by the parent not; a restart between turn end
   and report; the in-flight turn failed by `recover_missing_agent_after`.
4. **(a) Follow-up, stop and the end of the parent's session.** `delegation.follow_up`, `delegation.stop`, the mode check
   at a follow-up, a person's direct actions on a helper pane, stop on pane close, `workspace.close`, harness switch and
   session replacement. Tests: the same session continues; a refused follow-up on a stopped helper; follow-up when the
   running cap is full; `pane.close` of a helper; `workspace.close`; a lowered parent mode refusing a follow-up.
   **(b) Grants and refusals**, a security-review PR: the fingerprint, the `permission_decisions` table and migration,
   the answer path writing it, the lookup for a helper's request, `reject_always` only. Tests: a parent's
   `allow_always` covering a helper of another harness in a worktree; `allow_once` not covering; a helper's grant not
   reaching the parent; the later decision winning; `reject_once` not remembered; the 128 cap; the sentence added at
   `rule_key`. **(c) Idle release:** a `Hooks` method or clock, a released marker the reconciler honours, and the resume.
   Tests: a released helper resumed by a follow-up, and after an engine restart.
5. **Worktrees.** `WorktreePort::prepare` with a start ref; the parent in a worktree; the non-git error; the owner-generic
   checkpoint step; removal of an unchanged worktree and its branch with the `cleanup` checkpoint and its retry;
   `delegation.worktree.clean`; the kept limit; the orphan listing; Windows keeps all. Tests with real `git worktree`
   and the fake worktrunk; `tests/blocking_io_rule.rs` and `tests/core_lock.rs` green.
6. **The MCP tools and the acceptance.** The seven tools, the non-mutating list, the filtered `harness.list`,
   depth and `delegation.enabled` gating. `scripts/e2e_delegation.py` joins the e2e steps of the gate as
   `e2e_tasks.py` does (they run when the engine is selected); the Rust integration tests in `engine/soyeht-engine/tests`
   are full-tier. A full-tier real-harness check follows the pattern of `e2e_real_agent_messaging.py`.
7. **Helper types.** The kind in `soyeht-skills`, `helpers.*` and `helpers.trust`, the resolution, the user-wins rule, the
   MCP instructions for user types, the bounds, the allowed-tools enforcement, the type case added to the acceptance.
8. **The shared client and the Mac.** `DelegationReplica` and its vectors, the chip, the helpers view, Preferences for
   helper types, the check that the layout tolerates an unplaced pane, the size tokens; then the engine default of
   `delegation.enabled` becomes true. Tests: `swift test --package-path apps/shared`, the vectors, the Mac view checks.
9. **The phone.** 10. **Windows and Linux.** The same view and vectors.
11. **The task link.** `taskId` on `delegation.start`, with the contract-drift review.

**The acceptance** runs the engine with fake harnesses (`soyeht-fake-acp`). The cases:
- A parent calls `delegate` for two helpers of two fake harnesses in two worktrees and a third in `shared` isolation;
  the parent's turn ends and the helpers run together (the list shows two `running`).
- Each helper ends: the parent gets two attributed report prompts with the wrapper and the measured facts; a report
  whose text contains the closing marker and an instruction to "approve the pending request" changes no state.
- Helpers start in the parent's mode; a higher mode is refused; a fifth active helper is refused; `stop_delegate`
  stops the third; a harness that refuses `session/set_mode` fails its helper with no prompt sent.
- Follow-up after done: the parent has gone on to other turns, then follows up a helper that reported earlier; the
  transcript shows the same session continuing.
- Helper type by name: a trusted workspace `reviewer` type names the second fake harness, a plan mode and
  instructions; the helper's pane has them; a type naming a mode above the parent's fails the start; a user type of the
  same name wins.
- A helper raises a permission request: the parent's `pane.permission.answer` is refused; a person's answer is accepted.
  The person gives the parent an `allow_always`: a helper's identical request proceeds without a prompt, an `allow_once`
  does not carry, a grant on a helper does not reach the parent. A `reject_always` makes a second helper's identical
  request refused without a prompt and shows in the report.
- A helper calls `task_next`: refused; the helper's session is not restarted.
- A turn error scripted in the fake agent: the helper is `failed`, the report holds the error, a follow-up continues it.
  A turn that says a usage limit in text and ends: `done`, the text delivered as data, no classification.
- A waiting permission request expires after the 30-minute timeout (clock injected): the report says so.
- Session loss mid-turn: `failed`, the error in the report, the session alive where it is, follow-up allowed; an
  unrecoverable session: `session_lost`, no follow-up.
- The parent's queue is full: the report stays `queued` and is delivered when room appears.
- `pane.close` of a helper is a stop and wakes the parent; closing the parent's pane stops its helpers; `workspace.close`
  stops all. An unchanged worktree is removed, a changed one is kept.
- The engine is stopped and started in the middle of a helper's turn: the parent gets one report, not two.

## Open questions for the owner

The owner's answers of 2026-11-06 are rows 1 to 16 above. One question came out of the design review:

| # | Question | Recommendation |
|---|---|---|
| 1 | A repository can ship a helper type. Should a type from the repository be inert until a person approves its revision once (`helpers.trust`)? | Yes. A type chooses a harness, a model and instructions for an agent, so it deserves the confirmation a repository script would. It costs one click per revision, and the user's own types never ask |

## Consequences

- An agent of any harness with MCP can run helpers of any other harness in the background, with one mechanism for
  the Mac, the Windows and Linux app and the phone, and the person can name a type they defined.
- A helper can spend a subscription's quota without a person in the loop. The counts and the start rate bound the
  parallelism; the provider's own limit ends the rest, and the report says so when the harness reports it as an error.
- A repository can ship helper types. They can choose a harness, model and instructions, never a mode above the
  parent's, and only after the person trusts them.
- Contract 08's role matrix, the `Pane` schema, `x-limits`, `x-settings`, `x-modeRanks`, the `soyeht-skills` crate, the
  worktree port and the permission-answer path grow in the phases above. ADR 0029 and contract 03 R22 are amended for
  the worktrees a delegation makes. Contract 04 section 4 is amended for grants that cover helpers.
- The Core lock is untouched: worktree and git work run through `Outcome::Offload` and `soyeht_process::run`.
