# Pre-slice 9 fix batch · visible blocked states

Date: 2026-09-15. Profile: `Soyeht.next/dev`. License: Apache-2.0. Closing
implementation head: `5dc33a5bd1e37bbe62ed9592514e0bc8bdc2dcef`.

This batch is complete. A waiting agent turn now explains its state and offers
an exit, observed model refusals are attributed and remembered, the reported
usage-refresh rate limiting no longer reproduces under a correctly bounded log
window, and one pane's polling failure can no longer starve independent panes.
No production profile, tag or release was touched.

The shared rule is now concrete: when the system knows a state that blocks a
person, it presents that state in words on the surface where the person is
blocked. Live-only color, generic working state and unrelated error labels are
not treated as explanations.

## 1 · Agent turns with no output

### Measurement

The engine already had a 90-second `no_signal` state, but the Mac client showed
only the same danger color used for a dead pane. More importantly, the active
turn could be returned to generic `working` by frames that did not prove the
turn had progressed.

The discriminator was chosen from captured protocol traffic rather than from a
guess:

- a 30-second wedged turn emitted an available-command catalogue update and no
  prompt response;
- a healthy turn emitted the catalogue update, assistant content, accounting
  data and the prompt response; and
- a 180-second wedged turn emitted a catalogue update, accounting data and a
  final provider error response.

Any-frame activity was therefore false evidence of progress. Catalogue and
accounting updates alone cannot clear the wait.

### Change

Each accepted prompt now stores a per-pane `active_silent_since_ms` and starts
with no observed output. Parsed assistant message or thought content, tool and
plan updates, agent requests, or the prompt response count as turn activity.
Unknown updates remain non-progress while the presentation is non-destructive.
The state is durable and survives an engine restart.

The conversation shows `Waiting for the first agent output` from the beginning,
updates an elapsed clock each second and offers `Cancel turn`. At the existing
90-second boundary, the authoritative pane state escalates the wording without
cancelling, restarting or otherwise acting on the turn. A pending permission or
elicitation uses the separate `waiting_for_you` state. Cancellation clears the
durable wait, records `Turn cancelled by the user.` and leaves the pane and
process usable for a new prompt or harness switch.

An unavailable or cross-device-skewed clock never becomes a false `0m 0s`.
Elapsed time becomes unavailable while the true wait and Cancel remain visible;
the engine's `no_signal` state can still escalate the notice independently.

### Evidence and boundary

The automated proof covers restart restoration, two simultaneous panes with
independent clocks and cancellation, cancellation ordering and transcript
recording, a fresh prompt after cancellation, process ownership, permission
separation, and a healthy turn that emits output, stays quiet during a long tool
interval and resumes without a warning.

The reviewer drove the Dev app at `bad916c`: the notice appeared from the start,
the counter advanced, Cancel was available, and the notice cleared when the
turn ended. The measured provider turn ended at about 82 seconds, before the
90-second boundary. Escalation, cancellation while truly wedged, restart
restoration and the two-pane case therefore remain automated-test evidence,
not reviewer hands-on evidence.

A turn that emits meaningful output once and then wedges does not warn, because
observed output intentionally latches for the rest of that turn. This matches
the objective's protection for legitimate long silent tool work and remains a
known coverage limit.

## 2 · Honest picker and model-refusal states

### Measurement

Hands-on review found two related false presentations. A fetched agent or model
list briefly rendered as an empty search result while it was still loading. A
provider refusal appeared with the prefix `Internal error:`, making an account
or plan restriction read like an application crash. The model picker forgot
that refusal and offered the same choice without any marker.

### Change

The shared picker model now distinguishes three states: loading, a fetched but
empty catalogue, and no matches for a non-empty query. The new-pane composer,
live-switch sheet and model picker use that same state. A query that matches the
already-active agent explains that it is already active and asks the person to
choose another agent.

The engine learns only from an actual provider refusal. It writes a plain system
message that attributes the refusal to the agent, names the exact selected model
and preserves the provider's actionable text. Up to 256 exact harness/model
pairs are stored in schema 22 and returned through an additive protocol-v1
field. Pickers mark the exact refused model and keep it selectable; sibling
models and similarly named families remain unmarked. There is no pre-flight
availability guess.

### Hands-on evidence and boundary

On `1603f3c`, the reviewer saw `Loading agents…` and `Loading models…` on every
reachable picker, followed by the fetched list. A deliberately unmatched query
named the query instead of blaming an empty field. Search still reached the
intended agent. The empty-catalogue state remains test-only because the live
catalogue could not be emptied safely.

On `1ed4862`, the reviewer produced a fresh refusal and saw the exact model and
the provider's renewal instruction in the conversation without the internal
error prefix. Reopening the picker marked only that model as refused. A search
for the current agent explained that it was already active and what to do next.
The picker marker is intentionally short; the complete reason remains in the
durable transcript. Refusals observed before schema 22 cannot be reconstructed
and are marked only after a new refusal is observed.

## 3 · Usage refresh measurement

The initial report of 297 refreshes and 42 rate-limited outcomes was withdrawn.
It counted the entire non-rotating engine log, which contained 54 engine
lifetimes, and described accumulated history as a current run.

The repeatable window is the segment after the latest `engine.ready` boundary.
In the measured 10-minute-21-second run, the app made two `usage.refresh` calls:
one succeeded, one was unauthorized and none was rate-limited.

The reviewer then segmented the complete log by all 55 `engine.ready`
boundaries. Rate limiting appeared in runs 17, 41, 42 and 44–47, and in no run
after 47. Runs 48–55 made 135 refresh calls with zero rate-limited outcomes.
Call volume does not explain the difference: run 32 made 152 calls without rate
limiting, while run 44 made 160 with 35 rate-limited outcomes.

This item closes as **no longer reproducing**. No code changed, and no cause is
claimed. The log has no operation timestamps, so the clean transition cannot
be mapped to a commit. Unauthorized outcomes near earlier limited responses
make provider authentication state one possible explanation, but that remains
a hypothesis.

## 4 · Poll-loop failure isolation

[ADR 0068](../adr/0068-agent-poll-failure-isolation.md) replaces a public
error-kind allow-list with a boundary decision. Public error kinds describe
what a client receives; they do not say whether a failure belongs to one pane
or to shared infrastructure.

A missing supervisor instance retains its dedicated reconciliation path.
Storage durability, exhausted storage, supervisor transport and supervisor
generation failures stop the polling cycle. Every other failure raised while
polling an agent belongs to that pane by default: the engine durably marks the
pane dead, records the reason in its transcript and attention state, retires
its process and continues to the next pane. If that durable isolation itself
fails, the cycle stops.

The integration fault remains active on the lexicographically earlier pane
while a later pane receives and serves a real prompt. The test then proves that
only the healthy process remains and the failed pane's reason is durable. This
uses the same stable ordering that made the original starvation class appear
intermittent.

The safer default has an inverse diagnostic risk. A genuinely shared error that
is not classified as global, such as an unlisted non-exhaustion I/O failure,
can be attributed to one pane per cycle. Several panes dying close together
with the same reason is the signature of that misclassification. A future
instrumentation change should log the number of panes isolated in each cycle;
this does not change ADR 0068, but makes its failure mode visible.

## Continuation delivery check

Harness continuation is not sent through optional adapter metadata. Every
supported harness uses the same required ACP `session/prompt` path. The engine
opens the candidate session, applies requested options, sends the bounded
continuation package as the prompt body and commits the switch only after the
candidate accepts that prompt. A rejection, protocol failure or timeout retires
the candidate, restores the original session and records the failure durably.

The integration suite proves that the package reaches the candidate harness,
that a pending prompt is included exactly once, and that continuation failure
leaves the original session serving prompts. Hands-on slice-8 acceptance also
switched a live conversation between two installed harnesses and had the new
agent answer from context produced by the old one. There is no harness-specific
continuation branch that can silently ignore the package.

## Acceptance

| Acceptance | Result | Evidence |
| --- | --- | --- |
| 1 · visible silent wait | passed with named hands-on limits | Reviewer confirmed the initial notice, live counter, Cancel and clearing on turn end. Escalation and destructive-edge cases remain covered by automated tests because the live provider answered before 90 seconds. |
| 2 · plain disabled-model sentence | passed, reviewer confirmed | A fresh refusal rendered on screen with the exact model and actionable provider text; the reopened picker marked only that model. |
| 3 · usage refresh | closed without code | Eight consecutive engine lifetimes, 135 refreshes, zero rate-limited outcomes. The earlier report mixed historical runs; the cause of the clean transition is unproven. |
| 4 · poll loop decision | passed | ADR 0068 records the ownership boundary and the implementation test proves a persistently failing earlier pane cannot starve a healthy later pane. |
| 5 · no regression | passed | The closing implementation gate reports 573 test declarations, 37 automated acceptances and all 35 gate steps. Protocol generation, four hardening ratchets, Rust, shared client, Mac, iPhone and 810 TypeScript vectors pass. Protocol version remains 1. |
| 6 · restricted vocabulary | passed | The real 35-line private list was supplied locally from outside the repository. The scan was clean in `/Users/macstudio/Documents/soyeht-slice8` on implementation commit `93b1d2926b43964407d607495dfeef9654cffaa0`; the list and its contents were not committed or printed. |

## Local gate

The complete local gate ran with runtime tests serialized after one unrelated
parallel timing failure passed in isolation. The serial gate passed:

- 573 repository test declarations, up from 571;
- 37 end-to-end acceptances;
- all 35 gate steps, with no removed test, acceptance or verification step;
- generated protocol sources with no diff and protocol version 1;
- Rust formatting, clippy with warnings as errors, workspace build and tests;
- launchd end-to-end suites for engine, agents, tasks, editor, web and writing;
- shared-client tests and Mac and iPhone builds;
- 810 TypeScript protocol vectors;
- zero SQL outside the store;
- zero production panic sites;
- zero production functions above 120 lines; and
- font, color, editor-performance and restricted-vocabulary gates.

The report PR repeats this gate from the closing implementation base.

## Lessons and follow-ups

- A filtered absence is not evidence until the excluded frame or row kinds are
  named. The no-output discriminator came from comparing complete wedged and
  healthy frame inventories.
- A log without time cannot answer “when”, yet the investigation repeatedly
  asked it when behavior changed. Add a timestamp to each `engine.operation`
  event and rotate or truncate the engine log at startup early in slice 9.
- The item-4 isolation count described above would make a misclassified shared
  fault diagnosable before it kills several panes with the same reason.
- The self-recovering connection-interrupted modal remains an observation, as
  required by the objective. No work was opened on it in this batch.

## Delivery pull requests

1. [#282 · fix-batch objective](https://github.com/soyeht/soyeht/pull/282)
2. [#283 · restricted product-document copy](https://github.com/soyeht/soyeht/pull/283)
3. [#284 · durable visible no-output state](https://github.com/soyeht/soyeht/pull/284)
4. [#285 · unavailable and skewed wait clocks](https://github.com/soyeht/soyeht/pull/285)
5. [#286 · honest loading and empty picker states](https://github.com/soyeht/soyeht/pull/286)
6. [#287 · observed model refusals](https://github.com/soyeht/soyeht/pull/287)
7. [#288 · agent poll-failure isolation](https://github.com/soyeht/soyeht/pull/288)
