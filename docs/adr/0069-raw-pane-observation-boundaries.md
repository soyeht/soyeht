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

Metrics report precision and recall per state, pane kind, harness and ground-truth source,
detection latency and unknown rate. Terminal and agent panes are different populations and
are never aggregated, even when they use the same harness identity. Protocol bookkeeping and
a harness hook answer different questions and are never blended into one rate. A group with no
labels keeps an explicit absent source and reports scores as unavailable rather than disappearing
or contributing zeros to a total. This prevents dense labels from one source, harness or pane kind
from lending apparent evidence to another that has none.

Scores require twenty real ground-truth state transitions in their pane-kind, harness and source
group and twenty transitions into the individual state whose rate would be published. The initial
label and consecutive repetitions of the same state do not count as transitions. Twenty is a
descriptive floor declared before collection; it is not a confidence interval or a claim of
statistical significance. The group flag becomes available only when at least one precision or
recall rate is publishable. Each individual rate also requires its own denominator of at least
twenty, so one comparison can move a published rate by at most five percentage points. Below either
floor its numerator and denominator remain visible while its value is absent. The state floor
prevents a well-populated group from publishing a perfect-looking rate for a state represented by
only one or two episodes.

Collection is planned in transitions and comparisons, not elapsed recording time. Each
pane-kind, harness, source and state cell intended for recall needs at least twenty observations
under that truth state; precision additionally needs at least twenty detector predictions of that
state, which collection cannot manufacture. Working and idle can share twenty complete turn
cycles, while `waiting_for_you` requires twenty separate request episodes. Cancellation,
no-signal, recovery and death each require their own induced episodes if those cells are to be
published. A one-episode pilot supplies the duration for each scenario before the full collection;
twenty times that measured duration per required episode, plus setup, is only a lower bound for
the scripted session because ordinary use is slower and less regular. The floor is not lowered
after seeing how long collection takes.

Transition count alone does not prove independent evidence: a harness can flap between two true
states many times in one short burst. No duration cutoff is invented before observing that data.
Each state cell therefore publishes the minimum and median completed-state duration and the number
of entries lasting less than one second. Consecutive duplicate labels do not split a duration, and
the final open state is counted as unavailable rather than assigned an invented end. This
distribution exposes flapping without deciding in advance which duration is legitimate; a later
exclusion threshold must be declared with its measured reason before it is applied.
`waiting_for_you` is reported separately and never hidden by an aggregate because a
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
If export omits an incompatible segment, its interval cannot be recovered without decoding a
schema the current reader refuses. A scorer therefore marks the affected pane-kind and harness
group with the number of panes whose sources were omitted and makes elapsed coverage unavailable
for that whole group. Precision and recall remain available over retained labels, with the marker
making their truncated population explicit. Groups containing only complete panes remain usable.

### Separate, passive capture

Experiment recording is a new persisted per-pane value, separate from terminal replay
recording. It defaults off for every pane and can be enabled only in the Dev profile. The
pane shows recording state for the entire time it is active. Capture files remain beneath
the Dev profile in a dedicated experiment directory; no recorder, export or hook path is
available in another profile. Enabling capture does not imply export. Disabling capture
stops collection immediately and offers to erase that pane's existing segments. Pane deletion
does not erase exported datasets because one export may contain several panes; the confirmation
and receipt name that exclusion and the local exports directory. This slice adds no
whole-directory erase command; a person removes exported files from that stated local directory.
Deletion never follows symlinks, reports what was removed, and a failure remains visible rather
than being presented as successful withdrawal.

The same purpose-bound consent may enable a label-only recording on an agent pane. That
recording stores timestamped protocol states and harness provenance; it stores neither the
conversation transcript nor terminal text. The Mac UI names those different payloads in both
the consent prompt and the persistent indicator. Enabling an agent recording writes the exact
current protocol state at the consent boundary. A restored recorder does not manufacture that
label from a possibly stale checkpoint after an engine restart; it waits for the next durable
protocol transition. Disabling and deletion use the same signed-UI boundary and per-pane scope
as raw-terminal capture.

Official session hooks use a separate label channel. The engine adds bounded, inline hook
settings only while opening or loading a supported native session; it never edits the person's
shared harness configuration. The hook command receives the harness event on standard input,
keeps no event body, and submits only the closed event name plus a session capability. That
method is absent from the agent-facing MCP tool list. Its parameter parser rejects unknown
fields, so a caller cannot attach an alleged source, pane, harness or credential for the engine
to ignore or accidentally trust.

The engine mints the capability for one pane and native session, rotates it before every new
session, and rejects it after the pane closes. The configured hook remains an authenticated
no-op while experiment capture is off, so enabling consent later does not require restarting the
conversation and disabling consent writes nothing. On an accepted call the engine maps the
closed hook event to a pane state and stamps the live pane, harness, session generation, local
hook channel and capability digest itself. The raw capability is never written to the dataset or
conversation. This provenance is evidence about the connection and current engine state, never
a claim supplied by the payload.

An exact retry is resolved from its durable idempotency receipt before the engine looks up the
current session capability. A retry that arrives after capability rotation therefore returns the
original result without writing another label or attributing it to the replacement generation.
A new request carrying the retired capability has no receipt and is rejected as unknown.

The capability closes the convenient and accidental path by which an evaluated agent could
write its own labels. It is not a security boundary against a determined process running as the
same OS user: that process can inspect the session configuration or arguments and then imitate
the local hook client. The persisted channel, generation and credential digest make that route
auditable, but they do not prove that a same-account process did not copy the capability. This
residual risk is kept explicit rather than describing same-user secrecy that the platform does
not provide.

Only the Dev Mac application with a persistent signature may change experiment consent,
delete captured data or export it. At
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

The engine observes output only after the normal terminal stream has accepted it. The decoded
byte allocation is reference-counted between the retained screen tail and the capture handoff,
so enabling capture adds an ownership reference rather than copying the terminal payload. Before
this decision was implemented, the shipping poll path cloned that allocation for capture; removing
that product copy changed the cost being measured rather than merely correcting the fixture.
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

Before cadence measurement, the acceptable bound is fixed: each sample is the summed wall-clock
elapsed time for thirty complete `Core::poll_output` invocations, from each function entry to its
return, with capture-off and capture-on invocations interleaved against the same noisy-terminal
fixture. Over ten paired samples, capture may increase the median duration by at most 5 percent
and p95 duration by at most 5 milliseconds. The bounds are evaluated independently, so a fast
median cannot hide a long-tail stall. This measures the shared loop that recording could slow.
Terminal byte-to-client delivery is not the measurand because capture observes only a reference
after the normal stream has accepted the bytes and cannot replace or rechunk that path.

The cadence fixture is intentionally byte-heavy and frame-light: after separate unmeasured
engine and capture-path setup pairs, it retains ten raw paired samples. Each sample interleaves
thirty capture-off and capture-on polls, and each poll hands off one distinct VT block. Interleaving
keeps scheduler drift from landing on one complete series. The first three raw samples are
declared warm-up and discarded; the seven steady samples used for the acceptance bounds total
9,406,002 bytes and 210 projections. The complete raw fixture totals 13,367,794 bytes and 300
projections. If capture-on is faster than capture-off at either the steady median or p95, the
measurement is invalid and must be rerun rather than reported. The test permits at most ten
complete attempts; a valid attempt outside either performance bound fails immediately and is
never retried. A fixed clock and an empty in-memory local-server scan keep unrelated discovery
out of every retry. This isolates the per-byte shared handoff and bounded-channel cost that
remains on `Core::poll_output`. Test-only queue counters are disabled during the timed samples,
so the benchmark does not charge instrumentation that is absent from the shipping path. The
gate runs this measurement separately from concurrent engine tests, so unrelated scheduler and
disk load are not attributed asymmetrically to capture. During each timed sample, the test-only
projection worker is paused while the bounded queue accepts thirty frames, then resumed and fully
drained before the next sample. That keeps projection, serialisation, write and sync costs out
of the handoff measurement. It also deliberately removes producer-versus-worker
contention that exists in normal use, so this favourable isolation protects the synchronous loop
and bounded handoff rather than claiming a whole-system capture cost. The separate 200 Hz
worker-headroom fixture measures the per-frame projection, serialisation, write and sync costs;
neither result substitutes for the other. Before closed batching, that fixture retained 334 of
400 frames in two explicit gaps and measured 85.06 percent time coverage. Closed batches of up
to eight rows retained all 400 with no gap and 100 percent time coverage on the same fixture.
That 95 percent floor describes the gate's isolated, otherwise idle measurement condition. When
the same post-batching fixture ran beside the parallel Rust suite, which was the gate arrangement
until the isolated step was introduced, it retained 365
of 400 frames and reported 91.54 percent time coverage. The difference between the 91.25 percent
frame ratio and time coverage is expected because coverage is measured by elapsed interval. The
contended result remains part of the evidence because a developer machine is often busy; the
isolated result proves writer capacity without presenting it as a whole-machine guarantee.

The final full-gate sample with the interleaved estimator measured raw capture-off samples of
11015, 10679, 10731, 11059, 10410, 10200, 10476, 10793, 10767 and 10395 microseconds, and
capture-on samples of 10270, 11624, 10204, 10913, 10723, 10087, 10633, 10891, 10681 and 11292
microseconds. After the declared three warm-up samples, the median moved from 10476 to 10723
microseconds (+2.36 percent) and p95 moved from 11059 to 11292 microseconds (+233 microseconds).
Both independent bounds passed. Each sample contains thirty polls, so the median delta is roughly
8.2 microseconds per
`Core::poll_output`; that absolute observation is more durable than its percentage, because
unrelated baseline work changes the denominator. This replaces the earlier roughly 9 microsecond
observation because the shipping path stopped cloning the terminal payload; 8.2 microseconds
describes the new shared-allocation path rather than measurement drift in the old path. At the
new 30-poll sample scale, the 5 millisecond p95 allowance is about 45 percent of the capture-off
tail. It therefore guards both moderate tail drift and stalls; shrinking the sample back to one
poll would silently make that absolute criterion inert again. The 5 percent median bound remains
a coarse scheduling-sensitive guard: a failure requires investigation and is not by itself proof
that capture regressed. A future absolute-delta limit must be predeclared before the change it
evaluates rather than calibrated from this observed sample.

A later gate run under sustained machine load reported a 14.16 percent median increase from a
23.133 millisecond capture-off median. A
cross-revision control then alternated three separately built base and head binaries in one time
window. Base/head median increases were 4.29/0.99, 3.57/2.04 and 1.50/1.19 percent: no head
median regression was observed, and the head increase was below the base increase in every pair.
The corresponding base/head capture-off medians differed by +6.9, -0.7 and +19.9 percent. The
third head run also exceeded the independent tail allowance by 906 microseconds, with a 5,906
microsecond tail increase, while its capture-off median was 19.9 percent above the adjacent base.
These results do not identify the cause of the earlier 14.16 percent result, and no pair is removed
after seeing its outcome. For future cross-revision attribution, base and head capture-off medians
must differ by at most 5 percent to be called comparable. Drift larger than the effect allowance
means the environment changed enough to obscure the effect being attributed. This criterion was
declared after the three-pair control and applies only to future runs. Load average remains recorded
as context rather than replacing the capture-off arm measured by the same process and clock.

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
pane kind, harness identity and version, engine package version, digest of the installed engine
executable, and segment start time. Every observation and label row also carries an absolute
timestamp and a monotonic sequence for stable ordering when the wall clock changes. Rotation
happens before the next whole row is appended, repeats the metadata row and neither splits
nor drops a row. Protocol limits bound lines, line length, row size, segment size and retained
segments. Expired segments are removed oldest first. Schema version 6 adds pane kind to the
header, every data row, the clean-close marker and export provenance. A segment from another
schema is never assigned a pane population that it did not record. Replay omits only that
identified segment, reports its pane, schema version and count, and continues with compatible
segments and independent panes. The receipt tells the person to delete that pane's captured data
to remove the incompatible files. Malformed current-schema data still fails closed. A new
recording starts after the highest existing segment filename, including incompatible files, so
local omission cannot overwrite old data or collide with it.
Schema version 7 adds mandatory ground-truth provenance: the engine-observed channel, native
session generation and, for hook labels, the session-capability digest. Protocol labels carry
the protocol channel with no credential digest. Source, channel and credential shape must agree
or the label fails closed. Version 6 segments remain local omissions under the same typed
incompatible-source rule rather than receiving invented provenance.

Observation rows, label rows and overflow gap rows have distinct closed kinds. A gap carries
its exact available loss unit plus its first and last timestamps and sequences: projected
pressure records an observation count, while pre-projection pressure records bytes and leaves
the frame count absent. The two may coexist only when the writer itself coalesces both losses.
Queue pressure therefore cannot be replayed as a quiet terminal. Gaps feed neither the
detector nor ground truth.

Observations and labels obtain one sequence from the pane projection and enter one segment
writer, so file order cannot disagree with their shared sequence. Observation pressure may
collapse observations into an explicit gap. A label never enters that lossy path: its producer
waits until the pending gap is written and the bounded writer has room. Since the projection
worker is the sole producer, later observations cannot overtake a label whose sequence was
assigned. An invalid label is rejected before the sequence advances. A writer that neither
accepts the label nor fails within thirty seconds stops capture through the existing durable,
visible failure path, which names the writer delay and the elapsed bound. Thirty seconds is
declared before exercising the control: it is six hundred normal 50 millisecond batch windows,
leaving room for ordinary local-disk contention while bounding a recorder that can no longer
claim it is collecting. This deadline never infers pane state, cancels harness work or changes
terminal delivery.

ACP state transitions are protocol labels. The engine records one only after the corresponding
pane transition commits durably, and only when its source is `protocol`; a phrase-only update
does not create a duplicate label. The label-only recorder shares the same monotonic sequence,
rotation, retention and fail-visible writer as terminal observations. A live harness switch
cleanly closes the old harness segment before the first label opens a segment with the new
harness provenance. An official harness
hook may add labels only through that harness's documented, bounded child-process interface.
Hook configuration is scoped to the launched process and is never written into shared user
configuration. A harness without a supported official hook has no hook label; partial
coverage remains explicit.

An explicit additive engine command exports a caller-selected bounded time window to one
owner-only file beneath the Dev experiment directory. The request is bounded by pane, row and
byte limits, is idempotent, and is refused while any selected pane is still recording. The file
is installed atomically without following a destination symlink. Its first row states the
requested and actual window, every included pane and harness, the exact engine build, dataset
schema, source-segment sequence range and whether each source segment closed cleanly. An abrupt
source ending therefore remains visible instead of being presented as a complete recording.
The response returns the local path, row and byte counts, digest and the actual populated window;
an empty result omits that actual window instead of inventing one. Nothing is uploaded or copied
outside the profile. Analysis tools may read the returned local file. Only the attested Dev Mac
application can invoke export; command-line clients, agents and paired phones cannot. Export
rejects invalid provenance rather than manufacturing it. Incompatible source schemas are a
local, explicit omission in both the export header and receipt; one old pane cannot abort an
export of unrelated current panes.

An interrupted export may leave raw terminal text in its temporary file. At engine startup and
before a new export, the engine removes only a regular file whose name is exactly
`.export-<canonical UUID>.tmp` and whose modification time is at least 24 hours old. Startup
cleanup means an orphan does not depend on the person attempting another export. The age was
declared before exercising the control: a bounded 64 MiB export completes far inside that window,
while the window tolerates sleep, indexing, backup and a slow disk without deleting live work. A
future modification time has age zero and is preserved. Final `export-<digest>.jsonl` files and
every noncanonical neighbour are outside the match. The export response reports removed file and
byte counts; startup cleanup writes the same counts to the engine log. A startup cleanup failure
writes its reason to the log and does not prevent the engine from opening; an orphan is not grounds
to make the application unavailable. Cleanup failure during an explicit export still fails that
request. If cleanup succeeded but the export or its durable receipt then fails, the error still
states what cleanup removed; a privacy mutation never disappears behind the later failure.

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
