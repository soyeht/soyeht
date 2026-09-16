# Objective · Slice 9: the raw terminal and the instrument that decides its state

Self-contained, for execution to completion without questions to Caio. Same absolute
safeguards as slices 1–8 and the fix batch: no production, Dev profile `Soyeht.next/dev`,
no secrets, no tags/releases, no competitor brand names in product docs (vendored
integration catalogues are not our text), English-only repo, Apache-2.0, no AI attribution
in commits/PRs. Autonomy rule: ambiguity becomes an ADR and work continues; self-merge on
the green local gate; victoria reviews each PR after merge and exercises BY HAND, the same
day, anything a user can see. Do ALL git in your own clone, NEVER the main clone.
Date: 2026-09-15. Contract: `docs/contracts/05-pane-state.md`.

## 0. What this slice is, and the one thing it must not do

Contract 05 is deliberately PARTIAL. Caio decided on 2026-09-10 that the raw terminal's
state must be chosen **from data, not from taste**: record several harnesses, compare
candidate detectors against ground truth, then decide per harness.

**So this slice does not choose a detector. It builds the instrument that makes the choice
possible, and one honest baseline to beat.** If it ends with a clever classifier and no way
to prove it is better than a regex, it has failed. If it ends with a boring baseline and a
dataset that settles the question next week, it has succeeded.

Write that in the first ADR, because every later decision in this slice will be tempted
away from it.

## 1. Order of work

### 1.1 The recorder, and the fact that it decides everything downstream
Per contract 05 §4.1: on each screen change record `{pane, harness, harness version, t,
OSC title, last N lines, cursor position, foreground process, bytes per second}`. Rotated
JSONL, per pane, enabled per pane.

Three properties are not negotiable, and two of them come from a day we just lost.

**Every record carries an absolute timestamp.** Today `engine.log` cost us hours twice
because its operation events carry none: I counted 54 engine lifetimes as one run and
reported a number that was worthless, and neither of us could map "the rate limiting
stopped" to a commit. A dataset without time answers no question worth asking.

**The stream is rotated and every rotation is self-describing** — each file states the
pane, the harness, the harness version and the engine build that wrote it. A frame whose
provenance is unknown is not evidence.

**The recorder must never change what it records.** If enabling recording alters cadence,
buffering or what the user sees, the dataset describes the recorder.

### 1.2 Labels: the part that makes the data worth anything
Contract 05 §4.2. For ACP panes the protocol IS the label — reuse it. For harnesses with
official hooks, record timestamped hook events into the same stream as labels.

Two constraints. Hooks follow the harness's own contract (JSON on stdout, short) and are
**never written into the user's shared configuration** — contract 05 §3. And a label must
be distinguishable from an observation in the file: if a later reader cannot tell which
rows are ground truth, the dataset trains on its own guesses.

### 1.3 Export
Contract 05 §4.3: an engine command that combines frames and labels over a period into one
file for analysis outside the engine. It must state, inside the exported file, the window
it covers, the panes and harnesses included, and the engine build — the same provenance
rule as §1.1, because the export is what leaves the machine.

### 1.4 The detector interface and ONE baseline
Contract 05 §4.4: the engine calls `state(frame) -> (state, confidence)` per pane, and the
implementation is replaceable without touching the rest.

Ship exactly one implementation: the regex baseline drawn from the reference screen rules
and those already in the current Swift app. Not because it is good — because it is the
thing every later candidate must beat, and a comparison without a control proves nothing.

Do not ship the "changing screen" signal or a classifier in this slice. They are candidates
for the experiment, and a candidate that ships before the measurement becomes the default
by accident.

### 1.5 Metrics
Contract 05 §4.5: precision and recall per state and per harness, **with particular
weight on `waiting_for_you`**, plus detection latency and the `unknown` rate.

State the asymmetry in the ADR: missing `waiting_for_you` leaves a person blocked and
unaware, which is the exact failure the last batch existed to retire. A detector that is
excellent overall and weak on that one state is not acceptable, and an average will hide it.

### 1.6 The states reach the user in words
Contract 05 §1 and R12: one enum for both pane types, each state carrying `source`
(`protocol | hook | osc | screen | none`) and `confidence`.

`unknown` is the state no other app has, and it exists to stop us guessing. So it must
reach the person as a sentence, not as a colour: what the app does not know, and what they
can do anyway. We spent the last batch learning that a state the system knows and does not
say is the same as not knowing it — and `no_signal` sharing `dead`'s colour with no words
is precisely the bug we just fixed. Do not reintroduce it one pane type over.

Hold the three rules from contract 05 §3, and test each: never `idle` before the first sign
of life; `waiting_for_you` only with a real request, cleared by any new signal; partial
coverage surfaces as `unknown` and NEVER as `idle`.

## 2. Privacy — read this before writing the recorder

A raw terminal holds whatever the person typed: tokens, keys, private source, client names.
This slice writes that to disk by design.

Recording is per pane and OFF by default; the pane shows plainly, while it is on, that it
is recording; the data stays inside the Dev profile; export is an explicit act, never
automatic; and nothing is transmitted anywhere. If a decision here is not obvious, the safe
choice is to record less and say so in the ADR. Caio has not authorised recording anything
outside the Dev profile, and this objective does not either.

## 3. Acceptance

1. **[ADR]** The slice's purpose — instrument plus baseline, not a chosen detector — and
   the privacy boundary are recorded.
2. **[CI]** A recorded stream replays into frames with absolute timestamps and complete
   provenance; a rotation boundary loses no frame and each file is self-describing.
3. **[CI]** ACP protocol labels and hook labels land in the stream as labels, distinguishable
   from observations.
4. **[CI]** Export produces one file for a stated window with its provenance inside it.
5. **[CI]** The detector interface is swappable: a deliberately terrible stub replaces the
   baseline in tests without touching engine or client code.
6. **[CI]** Metrics compute per state and per harness against labels, and a control run
   shows the numbers DISCRIMINATE — a detector that always answers `unknown` must score
   badly, or the metric is measuring nothing.
7. **[CI]** The three state rules hold, each with its own red control.
8. **[reviewer, hands-on]** victoria drives a raw terminal pane in the Dev app: `unknown`
   appears in words with a way forward, recording is visibly off by default and visibly on
   when enabled, and a long-running command does not read as `idle`.
9. **[CI]** No regression across slices 1–8; four ratchets, font/colour gates,
   `generate.py --check` clean, protocol version still 1, any wire change ADDITIVE.
10. **[gate]** The restricted-vocabulary scan runs locally with the real list and is clean.

## 4. Carried over from the fix batch — do this early, it is small

Prove the slice-8 continuation package actually reaches the TARGET harness on every path we
support, not only the one it was developed against, and that a target which cannot consume
it says so rather than opening a session that quietly forgot everything. A published
orchestrator ships that exact bug: its handoff rides a field only 2 of its 13 adapters read.

## 5. Outside this slice

Choosing a detector; the "changing screen" signal; any classifier; the experiment itself
(that is Caio's, with data this slice produces); Windows and Linux; iPhone completion;
production. Also outside: recording anything in a profile other than Dev.

## 6. Delivery

Small PRs, one concern each, green local gate, self-merge, an ADR per ambiguity. Tell
victoria whenever you merge something with a visible surface and she will drive it the same
day. On completion, `docs/reports/slice-9.md` with the recorder's provenance format, the
baseline's measured numbers per harness, and — stated plainly — which questions the dataset
can now answer and which it cannot.
