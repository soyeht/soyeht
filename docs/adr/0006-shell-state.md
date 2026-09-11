# 0006 — Shell state without inferred readiness

Status: conservative interpretation for slice 1, 2026-09-11.

The objective lists process states for the first slice. Contract 05 is more
restrictive: partial coverage of a raw terminal appears as `não_sei`, never
as `ociosa`; readiness cannot be invented before a real signal.
This slice includes neither hooks, screen recognition nor the deferred experiment.

The engine uses `iniciando` during creation, `recuperando` while reconciling
the supervisor and `morta` after confirming exit. A live shell without a
verifiable readiness signal stays in `não_sei`, with source `none`, confidence
zero and the phrase "Shell active". Output silence is not used to declare idleness.

This preserves contract 05 and prevents a visual indicator from becoming false
permission to dispatch work. The terminal continues accepting human input.
Protocol agent states belong to the next slice. The discrepancy between the
objective's summary enumeration and this interpretation is in the final report.
