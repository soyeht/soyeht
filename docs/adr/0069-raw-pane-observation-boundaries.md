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
discriminates at both ends. Coverage is reported beside precision and recall for every state
and harness as the share of session wall-clock time outside explicit gap intervals. Time is
the unit retained observations, projected-frame gaps and raw-byte gaps all share; a byte gap
never becomes zero loss merely because its unknowable frame count is absent. A separate byte
ratio may describe transport loss, but it never substitutes for time coverage.
A dropped interval is attributed only where the surrounding ground-truth labels establish its
state; otherwise it remains unclassified coverage loss rather than being guessed. Candidate
results whose coverage differs by more than five percentage points are not ranked against one
another. This prevents queue pressure from flattering a detector by removing the busiest
intervals from its denominator.

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
the bytes sent to clients. `Core::poll_output` only offers that copy to a non-blocking,
32-chunk projection handoff. A dedicated worker owns the terminal parser and the recorder.
If this raw handoff fills, later bytes are coalesced into one gap carrying the exact byte count
and first and last timestamps. Its projected-frame count is absent rather than estimated.
The worker records that gap before accepting more bytes, resets the VT projection epoch rather
than applying later bytes to a parser that missed input, and then continues capture. Backpressure
is measurable loss, not recorder failure. A real worker or recorder failure stops capture,
publishes a durable visible reason and excludes that capture from candidate ranking.

After projection, a second non-blocking per-pane handoff holds at most 32 observation frames;
a dedicated writer performs segment writes and syncs. Before batching is changed, worker
headroom is measured separately from engine cadence with 400 visual changes delivered at
200 Hz. The predeclared acceptance floor is 95 percent time coverage at that frequency. Five
percentage points is already the boundary at which this ADR refuses to rank two candidates, so
loss beyond that amount makes the dataset unsuitable for the comparison it exists to support.
The claimed bound remains explicitly tied to 200 Hz rather than generalised to every workload.

The writer may sync a closed batch instead of every row. This changes crash durability: an
abrupt process death may lose the unsynced tail even when no in-memory queue saturated. Every
deliberately closed or rotated segment therefore ends with a synced clean-close marker. Replay
distinguishes a deliberate end from an abruptly ended segment; it never presents an absent tail
as a clean stop. A partial final JSONL row still rejects that segment under the existing
fail-closed rule.

On first saturation, every observation
still waiting in memory and the overflowing observation become one explicit gap, making that
gap the writer's next row rather than placing it behind the backlog. Further pressure extends
the same bounded row until the writer takes it. A storage operation that never returns cannot
promise crash durability, but once the writer can make one append, evidence of the loss lands
before later observations. Pressure therefore cannot be replayed as a quiet interval or wait
for saturation to end before leaving evidence. This deliberately discards up to 32 observations
that had already reached the in-memory queue. An opening gap row followed by stale frames and a
closing row would preserve more frames only when the process survives; a kill between the pair
would leave an unclosed gap and make the fail-closed reader reject the segment. Losing the bounded
queue is preferable to losing the file. Recorder failure disables experiment capture for that
pane and publishes a visible reason; terminal output continues. The experiment projection is
owned by the engine so collection does not depend on an attached or foreground client.

Before cadence measurement, the acceptable bound is fixed: the measurand is wall-clock elapsed
time for one complete `Core::poll_output` invocation, from function entry to return, using the
same noisy-terminal fixture with capture off and on. Over ten paired runs, capture may increase
the median duration by at most 5 percent and p95 duration by at most 5 milliseconds. The bounds
are evaluated independently, so a fast median cannot hide a long-tail stall. This measures the
shared loop that recording could slow. Terminal byte-to-client delivery is not the measurand
because capture observes only a copy after the normal stream has accepted the bytes and cannot
replace or rechunk that path.

The cadence fixture is intentionally byte-heavy and frame-light: after separate unmeasured
engine and capture-path setup pairs, it retains ten raw paired polls, each handing off one
distinct VT block. The first three raw pairs are declared warm-up and discarded; the seven
steady pairs used for the acceptance bounds total 308,135 bytes and seven projections. The
complete raw fixture totals 440,192 bytes and ten projections. If capture-on is faster than
capture-off at either the steady median or p95, the measurement is invalid and must be rerun
rather than reported. The test permits at most ten complete attempts; a valid attempt outside
either performance bound fails immediately and is never retried. This isolates the per-byte
copy and bounded-handoff cost that remains on
`Core::poll_output`. The gate runs this measurement separately from concurrent engine tests,
so unrelated scheduler and disk load are not attributed asymmetrically to capture. During the
timed pairs, the test-only projection worker is paused while the bounded queue accepts all ten
frames, then resumed before coverage is read back. That keeps projection, serialisation, write
and sync costs out of the handoff measurement; the separate 200 Hz
worker-headroom fixture measures the per-frame projection, serialisation, write and sync costs;
neither result substitutes for the other. Before closed batching, that fixture retained 334 of
400 frames in two explicit gaps and measured 85.06 percent time coverage. Closed batches of up
to eight rows retained all 400 with no gap and 100 percent time coverage on the same fixture.

On each projected screen change the engine records a bounded observation containing pane,
harness identity and version when known, an absolute timestamp, OSC title, the last bounded
screen lines, cursor position, foreground process and a bounded bytes-per-second measure.
Byte rate is not part of projected-screen equality: otherwise idle retention would fill with
near-duplicate frames as the rolling rate decayed. Instead, while bytes continue arriving
without changing title, lines or cursor, the engine records at most one static-activity
heartbeat per one-second window. No timer produces frames after bytes stop. This preserves
non-visual terminal activity for the experiment without rotating segments merely because a
pane is idle. Each row carries the closed emission reason `visual_change` or
`static_activity`; replay never has to infer what the writer knew by comparing adjacent rows,
which would be unsound across an explicit gap. The frame records the rolling byte rate at
either kind of emission. Unknown
metadata stays absent rather than being guessed. A raw terminal without a known harness keeps
an explicit unknown harness identity. Capture starts at the consent boundary and never
backfills the retained screen, because consent cannot authorise earlier terminal contents.

The emission reason is available to every experiment candidate because it is an observation,
not a ground-truth label. A candidate that uses only the emission reason is scored as its own
cheap baseline before it is combined with regex or learned features. Later comparisons must
therefore show what that signal is worth alone instead of crediting another candidate merely
for reading a precomputed field that all candidates received.

### Durable format and provenance

Each pane has rotated JSONL segments created with owner-only permissions and no followed
symlinks. Every segment begins with a metadata row containing the schema version, pane,
harness identity and version, engine package version, digest of the installed engine
executable, and segment start time. Every observation and label row also carries an absolute
timestamp and a monotonic sequence for stable ordering when the wall clock changes. Rotation
happens before the next whole row is appended, repeats the metadata row and neither splits
nor drops a row. Protocol limits bound lines, line length, row size, segment size and retained
segments. Expired segments are removed oldest first.

Observation rows, label rows and overflow gap rows have distinct closed kinds. A gap carries
its exact available loss unit plus its first and last timestamps and sequences: projected
pressure records an observation count, while pre-projection pressure records bytes and leaves
the frame count absent. The two may coexist only when the writer itself coalesces both losses.
Queue pressure therefore cannot be replayed as a quiet terminal. Gaps feed neither the
detector nor ground truth. ACP state transitions are protocol labels. An official harness
hook may add labels only through that harness's documented, bounded child-process interface.
Hook configuration is scoped to the launched process and is never written into shared user
configuration. A harness without a supported official hook has no hook label; partial
coverage remains explicit.

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
