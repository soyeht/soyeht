# Objective · New Soyeht slice 3: workspace tasks

Self-contained, for execution to completion without questions to Caio. Same
absolute safeguards as slices 1 and 2 (`00-slice-1-objective.md` §0 and §5: no
production, Dev profile `Soyeht.next/dev`, no secrets, no tags, no competitor
names, no assistant attribution, Apache-2.0). Same autonomy rule: ambiguity
becomes an ADR and work continues; self-merge with the green local gate;
victoria reviews after each merge. Date: 2026-09-11.

This slice must keep the hardening round 1 gates green (see
`docs/reports/hardening-1.md`): all task persistence lives in `soyeht-store`
as named functions with real-database tests (no SQL in the engine); new engine
effects go through ports with test doubles; no panic in production paths; one
error enum per crate mapped once to the wire; no production function above 120
lines; and no test, acceptance or gate step is removed. The four ratchets and
the restricted-vocabulary scan run before every merge.

## 0. Governing contracts for this slice
`03-workspace-tasks.md` (entire document, including the objects, the state
table in §3, dispatch and `task_next` in §5, blocking and CI feedback in §6,
review and the agent reviewer in §7, the attention inbox in §8, the per-pane
loop in §9, the lead agent in §10, the MCP tools in §11, the client events in
§12, the "Review applied" findings R07–R09, R13, R21–R23, the "must not
change" addendum and the contract-drift-review addendum),
`08-engine-and-clients.md` (task methods and events),
`05-pane-state.md` (pane state feeds task state phrases and the return rule),
`07-persistence.md` (worktrunk as the worktree layer; reconciliation with ptyd
before returning a task). Reuse the ACP agent pane from slice 2 unchanged for
task execution. Design: `design/soyeht-desktop.pen`: Screen 3 (board and list),
Screen 4 (task detail with the two drift lists) and Screen 10 (new-task dialog
with the required "must not change" field). The PENDING items in contract 03
take their assumed values: how-to-verify optional; PR contract off; agent
review on; loop limit 10, configurable.

## 1. What to build
1. **Protocol**: add to `contracts/protocol.json` (with vectors and generated
   types for Rust, Swift and TypeScript) the task objects — `Task`, `Subitem`,
   `Comment`, `Attempt`, `Blocking {kind, reason}`, `LoopConfig {prompt,
   interval, maxRounds, stopWhen}` — and the engine methods `task.create`,
   `task.next`, `task.show`, `task.list`, `task.update`, `task.comment`,
   `task.subitem.add`, `task.subitem.done`, `task.block`, `task.release`,
   `task.request_review`, `task.complete`, `task.dispatch`, `task.unblock`,
   `task.move` (drag), `task.worktree.clean`, `loop.set`; the events from §12
   `task.created`, `task.moved {actor, from, to, reason}`, `task.claimed
   {pane}`, `task.released {reason}`, `task.commented`, `task.subitem_changed`,
   `task.blocked {kind}`, `task.phrase_changed {phrase}`, `task.attention
   {n}`, `loop.tick`, `loop.stopped {reason}`; and the errors `transition_refused`,
   `task_has_owner`, `stale_attempt`, `stale_version`, `dependency_cycle`,
   `dependency_open`. `task.create` and `task.complete` reject missing required
   fields with `invalid_params` (see items 8 and 9). No method, event or field
   from slices 1–2 changes.
2. **`soyeht-store`**: task tables per workspace (`WS-n` ids), subitems,
   comments, attempts, move history, blocking, loop config, feedback
   fingerprints and counters. Every query is a named store function with its
   own real-database test. The atomic reservation for `task.next` is one store
   transaction: mark `in_progress` and `owner = pane` only if the row is still
   `todo`, ownerless and unblocked; return whether a row was affected.
3. **`soyeht-engine`, task rules behind ports**: the state machine of §3
   (refuse illegal moves with a reason: drag to `in_progress` and drag
   `todo → completed` both `transition_refused`; every accepted move records
   `{actor, from, to, reason}`); `task.next` accepted only from the calling
   pane's binding when the pane holds no other active task and is not closing
   or switching (R07), reserving the task, preparing the worktree through
   worktrunk, applying the preset, answering `{task, willRestartSession:
   true}`, ending the turn, then opening a new session IN THE SAME PANE in the
   worktree cwd with the task as its first prompt (R08); dispatch opening the
   pane, creating the worktree, binding pane and task, sending the first prompt
   (title, description, must-not-change, how-to-verify, subitems, comments,
   worktree), recording its steps and resuming or compensating after a crash
   instead of duplicating (R09); double dispatch refused with `task_has_owner`;
   the return rule only after reconciling with ptyd — a pane alive in ptyd does
   not return; `no_signal` beyond N minutes (N = 10, configurable) does not
   auto-return but enters the attention inbox; blocking as a FIELD (R13) with
   the three kinds of §6 (`dependency` returns to `todo` when the other
   completes; `missing_information` enters the inbox; `external_error` retried
   twice with delay, then `missing_information`); the state phrase computed
   from facts in the §6 priority order; every executor mutation carrying
   `{paneId, sessionGeneration, attempt}` and refused with `stale_attempt`
   unless it is the active attempt (R09); attempts closing with a result and
   unbinding the pane (R22); dependencies within one workspace, no cycles
   (`dependency_cycle` on creation), only `completed` satisfying them, a
   `cancelled` dependency leaving `blocked: dependency cancelled`; `task.update`
   requiring the current `version` (`stale_version` on conflict) (R23). New
   effects (clock, worktrunk, git/CI reader) go through ports with doubles.
4. **CI and review feedback (§6)**: when a task PR has red CI, a change
   request or a conflict, the engine delivers one message to the owning pane
   with a summary and log tail, once per fingerprint, at most 3 times per
   task, rearmed when CI turns green, and never over a permission dialog or an
   active human draft. The prompt queue is the durable per-pane queue of R21
   (origin, priority human > feedback > loop, `promptId`); loop ticks do not
   accumulate while busy or while the engine is stopped; counters and
   fingerprints persist.
5. **Review and the drift reviewer (§7, in v1)**: entering `in_review` shows
   the calculated badge SYSTEM'S TURN (CI red or changes requested, pane alive)
   or YOUR TURN (CI green awaiting a person, or no PR and review requested),
   and opens a second reviewer pane. That pane runs the contract-drift step of
   the addendum FIRST: it lists every public function, handler and protocol
   message the diff touched, assigns exactly one label per the taxonomy and
   severity order, and returns or approves per the outcome rules (incomplete
   always returned; the five contract labels returned unless declared in
   `contractChanges` with an updated contract/ADR; `extra_edits` returned
   unless in `outOfScopeEdits`; never approve on the promise of a later fix).
   The review comment carries one line per function `path::name → label → ok |
   returned: reason` and lands as a task comment the owner pane sees as YOUR
   TURN. The reviewer prompt and the candidate-pattern list come from the
   addendum.
6. **worktrunk integration (`07-persistence.md`)**: the engine drives worktrunk
   through its CLI/JSON to create a worktree per task on dispatch and on
   `task.next`, and to clean one on `task.worktree.clean` with the R22 checks
   (no process using it, clean tree, HEAD reachable from a branch outside the
   worktree; otherwise refuse with a reason). Branches are never deleted
   automatically. worktrunk is adopted, not reimplemented.
7. **Per-pane loop (§9) and lead agent (§10)**: `loop.set {prompt, interval,
   maxRounds, stopWhen}`; at each interval the engine sends the prompt only if
   the pane is idle; it stops on rounds exhausted, three consecutive empty
   `task.next` results, or an explicit off; the "3 empty" counter resets when a
   task is claimed. A pane with the `lead` role receives the create, dispatch,
   list, comment and unblock tools plus the role prompt of §10 and cannot move
   a task to `completed`.
8. **`task.create` and the required fields**: both the Screen 10 dialog and the
   `task_create` MCP tool carry title, description, **must not change**
   (required; empty → `invalid_params`), executor configuration, priority,
   worktree, permission and dependencies; history records the actor.
9. **`task.complete` and the two lists**: `task_complete {summary,
   contractChanges: [{path, name, label, contractRef}], outOfScopeEdits:
   [{path, reason}]}`; both lists may be empty but must be present; a
   completion without them is rejected with `invalid_params`.
10. **Test harness**: extend the deterministic fake ACP agent from slice 2 so
    it can, on instruction, claim a task with `task_next`, create subitems
    before executing and mark them, comment at milestones, block with a typed
    reason, request review, and complete with `contractChanges` and
    `outOfScopeEdits` (both well-formed and deliberately drifting). Add a fake
    CI/git port so PR check states and review states are scripted in tests. CI
    uses only these fakes. The optional local test (`SOYEHT_REAL_ACP=1`) uses
    the official Claude Code ACP adapter on Caio's Mac.
11. **CLI**: `task create`, `task next`, `task list`, `task show`, `task
    dispatch`, `task update`, `task comment`, `task subitem add|done`, `task
    block`, `task release`, `task request-review`, `task complete`, `task move`,
    `task worktree-clean`, `loop set`, mirroring the §11 tools for scripting and
    e2e.
12. **Mac app**: Screen 3 (board with the columns `todo`, `in_progress`,
    `in_review`, `completed`, and `cancelled` only when non-empty; a list view;
    drag between columns following the §3 rows, refusing the two forbidden
    moves with the engine's reason; per-card state phrase and priority; YOUR
    TURN / SYSTEM'S TURN badge on `in_review`), Screen 10 (new-task dialog with
    the required "must not change" field and the two create buttons), Screen 4
    (task detail: subitems, comments, attempts, blocking, the drift reviewer's
    per-function lines, and the `contractChanges` and `outOfScopeEdits` lists
    under the summary), the attention inbox of §8 with the "N need you" header
    and per-item "go to pane", and the per-pane loop and lead controls. The
    board is a view of the §12 events; it does not compute state. Neomorphic
    style from the `.pen`.
13. **CI**: everything from slices 1 and 2 plus this slice's tests; e2e with
    the fake agents running the task workflow under launchd.

## 2. Acceptance (work ends only when all pass)
1. [CI] Create in list through the dialog and through `task_create`: a task
   with an empty "must not change" is refused (`invalid_params`); a valid one
   appears on the board with `WS-n` and history "created by you|aria".
2. [CI] Dispatch: the engine opens a pane, creates the worktree through
   worktrunk, binds pane and task, and the first prompt contains the title,
   description, must-not-change, how-to-verify, subitems, comments and
   worktree; dispatching an already owned task is refused with `task_has_owner`.
3. [CI] `task.next` reservation is atomic: two idle panes race for one `todo`
   task; exactly one wins (`in_progress`, `owner`), the other gets the next or
   "empty"; the task is never executed twice.
4. [CI] `task.next` inside a loop turn (R07/R08): the reserved task restarts
   the session in the same pane in the worktree cwd with the task as its first
   prompt; the tool answered `{willRestartSession: true}`; the card then shows
   "in progress · attempt n"; never two executors.
5. [CI] Pane death returns the task: killing the executor pane mid-task returns
   it to `todo`, closes the attempt as "returned" and writes the system
   comment; a pane still alive in ptyd does NOT return; a pane in `no_signal`
   beyond N minutes does not auto-return and enters the attention inbox.
6. [CI] Subitems: the agent creates subitems before executing; the task enters
   `in_review` only when all are done; a task without subitems enters directly.
7. [CI] Typed blocking: a `dependency` block returns the task to `todo` when
   the other task completes and leaves "blocked: dependency cancelled" if the
   other is cancelled; `missing_information` enters the inbox; `external_error`
   retries twice then becomes `missing_information`. A blocked task cannot be
   claimed and shows "blocked: …" ahead of "available to claim".
8. [CI] CI feedback: a red PR check on a task keeps it in `in_review` with the
   SYSTEM'S TURN badge and delivers one message to the owning pane, once per
   fingerprint, at most three times, rearmed on green, never over a permission
   dialog or an active draft.
9. [CI] The drift reviewer runs on `in_review`: a diff with an undeclared
   `silent_fallback` is returned; the same drift declared in `contractChanges`
   with an updated contract/ADR passes; an `incomplete` diff is always
   returned; an `extra_edits` file is returned unless listed in
   `outOfScopeEdits`; `task_complete` without both lists is rejected with
   `invalid_params`; the review comment shows one `path::name → label` line per
   touched function.
10. [CI] State machine: dragging follows the §3 rows; dragging to `in_progress`
    and dragging `todo → completed` are both refused with `transition_refused`;
    every accepted move records `{actor, from, to, reason}`.
11. [CI] Safe retries (R09): an executor mutation carrying a stale attempt is
    refused with `stale_attempt`; a dispatch interrupted after "worktree
    created" resumes from the next step after restart and does not create a
    second worktree or pane.
12. [CI] `task.update` requires the current `version` (`stale_version` on
    conflict); creating a dependency cycle or a cross-workspace dependency is
    refused (`dependency_cycle`).
13. [CI] Per-pane loop: `loop.set` sends the prompt only while the pane is
    idle, counts rounds on actual dispatch, stops after three consecutive empty
    `task.next` results, the round limit, or an explicit off; ticks do not
    accumulate while the pane is busy.
14. [CI] Lead agent: a `lead` pane can create, dispatch, list, comment and
    unblock, and is refused when it tries to complete a task; its role prompt
    is applied.
15. [CI] Attention inbox: it lists `missing_information` tasks, `no_signal`
    panes, pending permissions, failed CI without a live pane and refused
    moves; the header shows "N need you"; each item reaches its pane.
16. [CI] Worktree cleanup is manual: `task.worktree.clean` refuses with a
    reason when a process uses the worktree, the tree is dirty or HEAD is not
    reachable from an outside branch; no branch is deleted automatically.
17. [local, procedure] On Caio's Mac with the official Claude Code ACP adapter:
    create tasks on the board, dispatch one to a real agent, watch it create
    subitems, comment and request review, and a reviewer pane run the drift
    step and return or approve; drag cards on the board; run a loop that pulls
    tasks with `task.next` until empty.

## 3. Outside this slice
The iPhone task board, dispatch and push (slice 8); harness switching and the
continuation package (slice 4); the browser and editor (slices 5 and 4);
cross-workspace board, automatic priority, event-driven recurring tasks,
sub-subitems and the PR contract (contract 03 §13, after v1); tasks created
from an issue or pull request (contract 03 open proposal, not a rule).

## 4. Workflow and delivery
Same as slices 1 and 2: order protocol → store → engine task rules and ports →
worktrunk → fake agents and CI ports → CI feedback and the drift reviewer →
CLI → e2e → Mac app → procedure; small PRs, green local gate with the real
restricted vocabulary and the four hardening ratchets, self-merge; an ADR for
each ambiguity; on completion, `docs/reports/slice-3.md` and a message to
victoria with the PR links.
