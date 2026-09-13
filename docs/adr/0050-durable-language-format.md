# ADR 0050: Durable language formatting

- Status: accepted
- Date: 2026-09-12

## Context

`file.format` is a mutation whose edits arrive asynchronously from a supervised
language server. The language-server response, the edited draft, and the
client's idempotency receipt cannot be acknowledged independently without
creating a crash window that either loses the result or applies it twice.

## Decision

The engine records the mutation scope, key, fingerprint, and normalized author
inside the durable language-request checkpoint before sending the formatting
request. This identity never crosses the language-server boundary. The raw
language response is checkpointed before the supervisor output is acknowledged.

After validating the returned UTF-16 edit ranges against the current revision,
one SQLite transaction saves the resulting draft, the public format result in
the language checkpoint, and the idempotency receipt. Only after that commit
does the in-memory editor adopt the draft and the waiting client receive the
result. The editor's existing autosave path remains the only path that writes
the draft to disk and records its history version.

A completed receipt wins over a recovered request. An unfinished response is
applied during language-server reconciliation after an engine restart. Once the
result is delivered or recoverable by its receipt, the engine removes the
language request and synchronizes the new revision back to the server.

## Consequences

An engine stop before the response leaves the durable request pending. A stop
after the response but before application leaves a recoverable response. A stop
after application leaves the draft and receipt committed together, so retrying
the same request returns the same file revision without running formatting
again. A changed file revision rejects the result without touching the draft.
