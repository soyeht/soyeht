# Slice 9, detector stage — objective

Released by Caio 2026-09-16. Written by victoria.

## The decision that shapes this stage

Caio: "podemos ter vários detectores e vamos vendo qual vai performando melhor
com o tempo."

This is not a bake-off that ends in one winner. **Several detectors stay in the
tree and are scored repeatedly against the same growing dataset.** Consequences,
and they are binding:

- `Detector` stays a trait with several implementations shipped, not one chosen.
- A run scores **every** registered detector, not the current favourite.
- Results must be comparable **across runs**, so each result carries the dataset
  fingerprint, the engine build, and the per-cell counts already required.
- Adding a detector must not require touching the ones that exist. A detector
  that can only be evaluated by replacing another is not a candidate.

## Blocker, before any number means anything

Observations are written only in `paneKind = terminal` (TerminalCapture) and
labels only in `paneKind = agent` (AgentCapture). `TerminalProjection::label`
has no production caller. Metrics group by paneKind, so observation and truth
never land in the same group: **no real recording can score any detector today.**
Only synthetic fixtures can, and a fixture is not a population.

Bridge this first. Three candidates, to be compared against one requirement —
same population, provenance demonstrable — and the cheapest does not win for
being cheapest:
1. the terminal recorder accepts labels;
2. the agent recorder accepts observations;
3. metrics join across populations.

Whichever is chosen, a real recording must end up containing observations and
labels that a reader can attribute to the same pane, same generation, same
harness, without inference.

## Candidates, from the benchmark — not invented here

`benchmark/estado-da-pane.md` studied five apps that ship this feature, in
their code. The ranked recipe it produced:

1. **Hooks first** in the three audited harnesses. Already built for Claude, session-scoped.
2. **Event → state table**, copied from the audited event model, including `AskUserQuestion =
   waiting for you`.
3. **OSC marker** as a second channel, for harnesses that emit one.
4. **Regex only as fallback**, per-harness manifest, always marked "advisory".
5. **"No signal"** with a 90 s grace, only for harnesses with full coverage.

Field evidence worth carrying: state/hook bugs are the **most common bug
category** in every audited app: 204 issues of ~4,900, 353 of 5,345, 185 of the
last 1,000, 29 of 310, and 23 of 413. The per-app attribution stays in the
benchmark, outside this repository. One audited product rebuilt its status
reporting around what a harness can honestly report, precisely because
screen+timer guessing failed in **both** directions: agents stuck on "working"
after finishing, and false "done" during a long silent tool run.

## Eight constraints the field already paid for

From `estado-da-pane.md`, lessons drawn from issues rather than code:

1. Sticky state needs an exit: clear on any new signal in the same turn, and
   never hold "waiting for you" without a real open request.
2. Distinguish "asked permission" from "finished and went quiet" — the same
   Claude Code notification serves both.
3. Never declare "idle" before the harness's first sign of life.
4. Do not write hooks into the user's shared configuration; scope per pane.
5. A hook must obey the harness's contract or it breaks the agent, not just
   the state.
6. Partial hook coverage must surface as **"I don't know"**, never as "idle".
7. Regex breaks every release: fallback only, advisory, versioned manifest.
8. Engine separate from the app avoids the zombie class one audited harness hit.

## What a result must say

Per `paneKind × harness × source`, with the floors already declared: 20 real
transitions per group, 20 real entries per state, `denominator >= 20` before a
rate publishes a value. Below that, `scores_available = false` and the counts
still visible.

And three statements that must never be merged into one number:
- precision and recall over retained labels;
- time coverage, or its absence when a source was omitted;
- which label source produced the truth, because a screen detector scored
  against protocol truth answers "can the screen reproduce what ACP already
  says", not "is the detector right".

## Not in this stage

Choosing a winner. Shipping a notification. Changing what the user sees. This
stage produces measured candidates and the machinery to keep measuring them.
