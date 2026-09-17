# Frame-dense headroom anomaly · 2026-09-17

Status: open instrument issue. The single severe failure did not follow the
candidate artifact in a later paired control, but its cause is unknown.

The isolated full gate on candidate `aa2dde60c380411f81117cfb15339fbd25a5b5ad`
stopped after retaining 319 of 400 observations. It reported one gap, 1,539
dropped bytes, peak outstanding work of 33 and 8,009 basis points of time
coverage. The gate was not retried for approval.

The candidate change was outside the fixture's reachable path. Normalized
instruction streams were identical between base and candidate for the fixture,
`TerminalCapture::record`, `projection_loop`, `TerminalProjection::process`,
`ObservationRecorder::try_record` and `SegmentStore::append`. Cargo manifests,
dynamic dependencies, registered tests and static initializer shape were also
unchanged. The candidate test binary was 1,312 bytes larger because of server
code that this fixture does not call.

Ten subsequent base runs all retained 400 observations with no gap or dropped
byte and full time coverage. A prospectively declared ten-pair AB/BA control
then alternated separately built base and candidate artifacts. Every one of its
twenty arms retained 400 observations with no gap or dropped byte and full time
coverage. Peak outstanding work was 4–5 with median 4 for both artifacts. The
largest within-pair difference in one-minute load was 1.86 percent, below the
prospective 20 percent comparability limit. No pair was removed or repeated.

A separate ten-pair process-start control produced median startup times of
8.644 milliseconds for base and 8.622 milliseconds for candidate; the median
paired candidate-minus-base delta was 0.053 milliseconds. Its cold first pair
is retained in the raw data.

These controls remove the candidate from the observed failure's attribution;
they do not explain the failure or turn the original gate result green. A
rare, severe queue-pressure mode remains possible in the fixture or the
machine. Its frequency is unknown. Future occurrences must preserve the full
sample rather than being retried away.

Evidence:

- `main-results.jsonl`: ten base-only characterization runs, including load
  and Dev-engine CPU sampled for each run.
- `headroom-manifest.json` and `headroom-results.jsonl`: the predeclared AB/BA
  order, artifact digests and all twenty headroom arms.
- `startup-manifest.json` and `startup-results.jsonl`: the same for process
  startup.

The original failing gate output was not retained as a standalone artifact;
the exact values above were copied from that gate result before investigation.
That missing raw file limits diagnosis and is part of the open issue.
