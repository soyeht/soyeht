# Objective · Fix batch before slice 9: every blocked state must be visible

Self-contained, for execution to completion without questions to Caio. Same absolute
safeguards as slices 1–8: no production, Dev profile `Soyeht.next/dev`, no secrets, no
tags/releases, no competitor brand names in product docs (integration names such as the
ACP registry entries are integrations and may stay — Caio decided this on 2026-09-15),
English-only repo, Apache-2.0, no AI attribution in commits/PRs. Autonomy rule: ambiguity
becomes an ADR and work continues; self-merge on the green local gate; victoria reviews
each PR after merge. Do ALL git in your own clone, NEVER the main clone. Date: 2026-09-15.

## 0. Why this batch exists, and why it comes before slice 9

Caio decided on 2026-09-15: close the known defect list BEFORE opening slice 9. Each new
slice has been adding surface faster than the end-of-slice pass finds defects, so the
defects arrive together, in his hands, at the worst moment.

This batch is not a list of unrelated bugs. Four unrelated subsystems produced the SAME
defect in a single day: a delivery receipt that could not distinguish a whole message from
a truncated one; a send-time status read as an outcome; a failure that existed only as a
live event and vanished on restart; a pane that queued prompts with no agent behind it.
In every case the system knew something that blocked the person, and the person could not
find out.

**The rule for this batch, above any individual fix: every state the system can be in that
blocks the user must be visible to the user, in the surface where they are blocked, in
words a person can act on.** A fix that repairs one instance and leaves the class open has
not met the objective. Prefer one mechanism that makes blocked states observable over five
special cases.

## 1. Order of work

### 1.1 A pane that is waiting on an agent that has produced nothing (FIRST — highest pain)

Measured: Caio's configured default model (an expired plan) does not refuse a prompt — it
never answers. The pane sits in its working state forever. No elapsed time, no explanation,
no way out short of closing the pane. This cost him a full session and reads as "the app is
broken" because nothing distinguishes it from a slow answer.

Required observable: when a prompt has been accepted and the agent has produced NO output
of any kind since, the pane shows that it is waiting, shows how long, and offers at least
one way out that does not destroy the pane.

Two things this must NOT become. It must not be a timeout that cancels legitimate long
work — a long turn that is streaming is healthy and must not be touched. And the signal
must key on "nothing at all has arrived since the prompt", not on "this is taking a while".

### 1.2 A model the user's plan disables

Measured: Caio's account has 165 models on one harness. Some work (`big-pickle`); others
answer a prompt with JSON-RPC -32603 and the agent text "Model is disabled". Today that
reaches the transcript as a system row reading `Internal error: Model is disabled`, and the
picker gives no hint beforehand.

Required observables, in this order of value:
1. The refusal renders in the pane in plain words, naming the model, not as `Internal
   error: …`. The user must be able to tell "your plan does not allow this model" from
   "the agent crashed".
2. Once a model has been observed to be refused for a harness in this profile, the picker
   marks it. Not removed — marked, with the reason.

Honesty boundary: we have NOT demonstrated any way to know a model is disabled BEFORE
sending a prompt, and the objective does not ask for one. Do not add a pre-flight probe
that guesses. Learn from the refusal we already receive.

### 1.3 `usage.refresh` is still rate-limited at scale

Measured on the slice-8 closing build: one sample showed 297 `usage.refresh` calls with 42
`rate_limited` outcomes. Caveat I am obliged to state: my pre-#264 and post-#264 samples
used different window definitions, so I have NO clean before/after ratio — only "still
happening, in quantity".

So this item begins with a measurement you can repeat, not with a fix. Define one window
(proposal: the app idle, one workspace open, ten minutes, counted from the engine log) and
record calls and `rate_limited` outcomes on today's main. Then fix, then re-measure the
SAME window. The PR must carry both numbers. If the first measurement shows the problem is
already gone, say so and close the item — that is a valid outcome and I will not treat it
as work avoided.

Direction, not prescription: a per-provider ceiling on refresh rate, coalescing concurrent
refreshes into one in-flight call, and backing off after a `rate_limited` rather than
retrying into it.

### 1.4 `Core::poll_output`: the per-pane error allow-list must become a decision

In `poll_output`, exactly two error kinds are handled per-pane (`InvalidParams`,
`HarnessUnavailable`); every other kind returns from the loop, which starves every pane
sorted after the failing one. This is not a regression and I have not observed it failing
in production use. But it is the exact shape of both bugs the slice-8 gate caught (#278,
#280), and it is currently an accident rather than a choice.

The deliverable is an ADR, and whichever code follows from it: either invert the list (fail
the pane for every kind except a named few that are genuinely global), or state why the
current two are right and what makes every other kind un-recoverable per-pane. Either
answer is acceptable. Leaving it undecided is not.

## 2. Acceptance (the batch ends only when all pass)

1. **[hands-on, victoria] The silent wait is visible.** With a model that accepts a prompt
   and never answers, the pane shows a waiting state with elapsed time and an escape, and a
   healthy streaming turn of similar length shows no such warning. I drive both by hand.
2. **[hands-on, victoria] The disabled model reads as a sentence.** Sending to a refused
   model renders a plain-language message naming the model, on screen, in the pane — not
   only in the durable store. Re-opening the picker marks that model.
3. **[CI + evidence] Usage refresh.** Same-window before/after counts in the PR body, from
   the engine log, with the window definition stated.
4. **[ADR] Poll loop.** The allow-list is documented as a decision with its reason, and the
   code matches the decision.
5. **[CI] No regression.** Every slice 1–8 acceptance/test passes; four ratchets +
   font/color gates green; `generate.py --check` clean; protocol version still 1; any wire
   change is ADDITIVE.
6. **[known gap, accepted]** The restricted-vocabulary scan cannot run until Caio supplies
   its private input. Caio knows and it is his to unblock. Report it as NOT RUN. Do not
   report the gate as green, and do not infer that it would pass.

## 3. Outside this batch

Slice 9 (raw terminal) — it starts when this batch closes. A pre-flight probe for model
availability. Any change to the production Soyeht app: the 672-byte text clipping in the
CURRENT app belongs to anastasia and is not yours. Reworking the usage rings visually. The
self-recovering "Connection interrupted. Reconnecting…" modal stays an observation — log
occurrences if you see them, do not open work on it.

## 4. Workflow and delivery, and the new review cadence

Small PRs, one item each, green local gate, self-merge, an ADR per ambiguity.

**What changes for you:** victoria now exercises BY HAND every PR that touches anything a
user can see, on the day it merges — not only at slice close. Caio decided this on
2026-09-15. When you merge something with a visible surface, say so and I will drive it the
same day. Expect more interruptions from me, earlier, and smaller. The three findings from
the slice-8 hands-on pass would each have surfaced alone and cheap, the day their PR
landed, instead of arriving together at the end.

On completion: `docs/reports/fix-batch-before-slice-9.md` with, per item, what was measured
before, what changed, and what was measured after — and the two items whose proof is a
hands-on observation marked as mine to sign off, not yours.
