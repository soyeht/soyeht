# ADR 0069: raw pane observation boundaries

Status: accepted
Date: 2026-09-16

## Context

Contract 05 deliberately leaves raw-terminal state detection undecided. The next decision
must come from comparable observations and ground-truth labels across harnesses, not from a
preferred heuristic. Terminal contents can contain credentials, private source and client
data, so producing that evidence also creates a new sensitive-data boundary.

The existing pane `record` value controls durable terminal-byte replay. It is enabled for
some existing flows and has different retention and recovery semantics. Treating it as
consent to an experiment would expose data for a purpose the person did not choose.

## Decision

### Instrument before detector

This slice delivers an observation instrument and one regex baseline. It does not select a
detector. A pure pane-state crate owns the bounded frame and label types, the replaceable
`state(frame) -> (state, confidence)` interface, the single baseline, replay and metrics.
It has no filesystem, process, socket, engine or client dependency. A deliberately poor
detector can replace the baseline in tests without changing engine or client code. Screen
change and learned classifiers remain experiment candidates and do not ship in this slice.

Metrics report precision and recall per state and per harness, detection latency and unknown
rate. `waiting_for_you` is reported separately and never hidden by an aggregate because a
miss leaves a person blocked. An always-unknown control must score poorly, and a
perfect-oracle control must reach the metric ceiling. Together they prove the scoring
discriminates at both ends.

### Separate, passive capture

Experiment recording is a new persisted per-pane value, separate from terminal replay
recording. It defaults off for every pane and can be enabled only in the Dev profile. The
pane shows recording state for the entire time it is active. Capture files remain beneath
the Dev profile in a dedicated experiment directory; no recorder, export or hook path is
available in another profile. Enabling capture does not imply export. Disabling capture
stops collection immediately and offers to erase that pane's existing segments. A separate
explicit erase action removes the entire experiment directory, including exports. Deletion
never follows symlinks, reports what was removed, and a failure remains visible rather than
being presented as successful withdrawal.

Only the Dev Mac application with a persistent signature may change experiment consent. At
Unix accept time the engine obtains the peer endpoint's kernel audit token, which identifies
the process instance and cannot be confused by PID reuse. Security.framework resolves that
audit token and validates the live code in process against the application identifier and the
engine's own Apple Development team. No path, PID or external command mediates the decision.
This attests which program created the accepted peer endpoint; it does not claim that the engine
can prove which physical person acted. A Unix stream does not re-attest individual writes after
a deliberate descriptor transfer, so this boundary also requires the UI to keep its client
socket private; the app has no descriptor-delegation path. Same-UID command-line clients, bound
agents, raw socket clients, paired phones, ad-hoc builds and a signed app from another team fail
closed. Refusing the paired phone is deliberate policy: consent to record a Mac terminal is
granted at that Mac, by the person in front of it. Stable signing is part of this boundary;
reverting either binary to ad-hoc signing makes consent changes unavailable rather than silently
weakening the check.

The engine observes a copy of output only after the normal terminal stream has accepted it.
It never delays an acknowledgement, changes chunking, changes screen cadence or replaces
the bytes sent to clients. Recorder failure disables experiment capture for that pane and
publishes a visible reason; terminal output continues. The experiment projection is owned
by the engine so collection does not depend on an attached or foreground client.

On each projected screen change the engine records a bounded observation containing pane,
harness identity and version when known, an absolute timestamp, OSC title, the last bounded
screen lines, cursor position, foreground process and a bounded bytes-per-second measure.
Unknown metadata stays absent rather than being guessed. A raw terminal without a known
harness keeps an explicit unknown harness identity.

### Durable format and provenance

Each pane has rotated JSONL segments created with owner-only permissions and no followed
symlinks. Every segment begins with a metadata row containing the schema version, pane,
harness identity and version, engine package version, digest of the installed engine
executable, and segment start time. Every observation and label row also carries an absolute
timestamp and a monotonic sequence for stable ordering when the wall clock changes. Rotation
happens before the next whole row is appended, repeats the metadata row and neither splits
nor drops a row. Protocol limits bound lines, line length, row size, segment size and retained
segments. Expired segments are removed oldest first.

Observation rows and label rows have distinct closed kinds. ACP state transitions are
protocol labels. An official harness hook may add labels only through that harness's
documented, bounded child-process interface. Hook configuration is scoped to the launched
process and is never written into shared user configuration. A harness without a supported
official hook has no hook label; partial coverage remains explicit.

An explicit additive engine command exports a caller-selected bounded time window to one
owner-only file beneath the Dev experiment directory. Its first row states the requested and
actual window, every included pane and harness, the engine build and the dataset schema.
Nothing is uploaded or copied outside the profile. Analysis tools may read the returned local
file. Export rejects incomplete provenance rather than manufacturing it.

### State claims

The baseline can produce only claims supported by its frame. A raw terminal never becomes
`idle` before a real sign of life, `waiting_for_you` requires a real request and any later
signal clears it, and partial coverage yields `unknown`, never `idle`. Every state retains
its source and confidence. The Mac client renders the state in words; `unknown` says what is
not known and what the person can still do.

Ground-truth labels never feed the live detector. They exist only for replay and scoring, so
the measured candidate cannot grade itself. Unknown frame or label variants fail closed in
the recorder and replay parser.

### Continuation proof

All supported ACP harness recipes use the same target `session/prompt` path for a harness
switch continuation package. The switch commits only after the target acknowledges that
prompt. Tests exercise every supported launch-recipe kind through that path and an induced
target refusal. A target that cannot consume the package leaves the original session usable
and records a durable user-visible failure; it never opens a fresh session that silently
lost context.

## Consequences

Terminal replay and experiment consent cannot be confused. The dataset is attributable to
an exact engine binary, survives rotation without ambiguous rows, and can be reproduced by a
headless engine without changing the terminal being measured. The first baseline is useful
as a control rather than as a product decision. Raw data remains sensitive even with these
bounds, so deleting the Dev profile deletes the only automatic copy and export remains an
explicit local act.
