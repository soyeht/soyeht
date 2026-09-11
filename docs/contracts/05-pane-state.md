# Contract 05 · Pane state

Status: PARTIAL, decided by Caio on 2026-09-10. The agent pane (ACP) is
settled. The raw terminal pane awaits a data-driven EXPERIMENT;
this contract defines what the engine must prepare for that experiment.
Source: (internal analysis, outside the repository) (internal analysis).

## 1. States clients receive (both pane types)
`trabalhando`, `ociosa`, “waiting for you” (permission or question), “no
signal”, `morta`, and **“unknown”** (only in the raw terminal pane when
confidence is low; no other app has this, and it prevents guessing).
The card phrase (contract 03 §6) is calculated from this.

## 2. Agent pane (ACP) — SETTLED
Comes from the protocol (contract 04 §8): accepted prompt → `trabalhando`;
open `session/request_permission` → “waiting for you”; `idle` → `ociosa`;
no notification for 90 s with an open turn → “no signal”; process exited →
`morta`. Exact; no screen reading.

## 3. Raw terminal pane — DECISION DEFERRED TO AN EXPERIMENT
Caio's decision: do not choose between a hook, marker or regex now. Decide
using data, like a data scientist, after measuring several harnesses.
His hypothesis: a lightweight classifier over the screen should work better
than handwritten rules (“like email: rules did not work, a classifier did”).
Simple starting signal: changing screen = working; static screen = stopped.

Rules already in effect, drawn from others' mistakes (issues):
- never declare `ociosa` before the first sign of life; `task_next` and the
  loop send prompts only after the first real `ociosa`;
- “waiting for you” only with a real request; any new signal clears it;
- partial coverage appears as “unknown”, never as `ociosa`;
- if hooks are introduced, they follow the harness contract (JSON stdout,
  short duration) and are not written into the user's shared configuration.

## 4. What the engine must prepare for the experiment (included in v1)
1. **Pane frame recording**: on each screen change, store
   `{pane, harness, harness version, t, OSC title, last N lines,
   cursor position, foreground process, bytes per second}`.
   Format: rotated JSONL per pane; enabled per pane (“record for experiment”).
2. **Ground-truth label**: for harnesses with official hooks (Claude Code,
   Codex), record timestamped hook events in the same stream as labels
   (“started”, “called tool”, “requested permission”, “stopped”).
   For ACP panes, the protocol itself provides the label.
3. **Export dataset**: an engine command combines frames + labels from a
   period into a file for analysis outside the engine (Python).
4. **Detector interface**: the engine calls a per-pane function
   `state(frame) -> (state, confidence)`; its implementation may be regex
   (baseline: reference screen rules and those in the current Swift app), the
   “changing screen” signal, or a classifier; replaceable without changing the rest.
5. **Metrics**: precision and recall by state and harness, emphasizing
   “waiting for you”; detection latency; “unknown” rate.

## 5. Experiment plan (later, with Caio)
Record hours from several harnesses; compare reference screen rules, current
app rules, the “changing screen” signal and a lightweight classifier against
the labels; decide per harness. Learn from issues: “notification with two meanings”,
“sticky state”, “background shell”, “permission prompt that looks idle”.

## Review applied (2026-09-10, finding R12)
One enum for both pane types: `iniciando`, `trabalhando`, `ociosa`, `esperando_você`, `cancelando`, `sem_sinal`, `recuperando` (engine reconnecting to ptyd), `morta`, `não_sei`. Each state carries `source` (`protocol | hook | osc | screen | none`) and `confidence` (0–1). `esperando_você` requires an open request; `sem_sinal` neither clears a request nor releases the loop/`task_next`; `iniciando` never becomes `ociosa` without a real signal.
