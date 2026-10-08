# ADR 0044 · Usage refresh triggers

Date: 2026-09-12
Status: superseded by ADR 0077 (2026-10-03)

## Context

Contract 10 gives the Mac app ownership of rail visibility and hover state. It
also requires refreshes while the rail is open and on hover. The wire method
contains only an optional provider and an idempotency key, so the engine cannot
identify either presentation trigger without adding an uncontracted field.

A refresh without a provider targets the whole ordered registry. The contract
limits calls per provider but does not define whether a partly limited batch
may read the remaining providers.

## Decision

The Mac app observes its local rail and hover state and requests refreshes at
the contracted times. The engine remains the only component that invokes a
reader. It applies the per-provider 30-second limit, records transient backoff,
normalizes results, persists successful readings and publishes every outcome.
The app never reads an account source and never computes whether a reading is
authoritative.

A batch request is all-or-none at the reader boundary. If any selected provider
is inside its 30-second limit, the engine reads none and returns `rate_limited`
with the longest remaining wait. This prevents a retry from refreshing an
unintended subset and gives the caller one time after which the entire request
can run.

## Consequences

Rail and hover scheduling can be tested as client navigation behavior while
reader access and rate limiting remain engine tests. The existing protocol is
unchanged. A future shared schedule would require an explicit contracted
client-interest method rather than inference from connection state.
