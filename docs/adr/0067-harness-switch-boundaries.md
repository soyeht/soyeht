# ADR 0067: harness switch boundaries

Status: accepted
Date: 2026-09-15

## Context

An agent pane currently binds one native session to one harness for its lifetime. Changing
model or effort may preserve that session, but changing harness cannot: native session
identifiers, configuration and authentication belong to the harness that issued them. The
catalogue also contains more entries than a person can scan as one alphabetical list.

## Decision

A harness switch replaces the native session while preserving the Soyeht pane, normalized
transcript, pane file, task and workspace identity. The engine serializes the operation per
pane and freezes its prompt queue. It cancels an active turn, builds a continuation package
from the last six complete normalized user or assistant records, including frozen queued
prompts tagged as pending data, and starts a candidate session in the same working directory.
The old process and session remain available until the candidate accepts its first prompt.
Only that acknowledgement commits the new harness, records a visible restart marker,
removes incorporated pending prompts from the resumed queue and retires the old session.
Any earlier failure discards the candidate and resumes the original queue and session;
pending prompts are not lost or duplicated and the pane never becomes agentless.

The continuation builder is deterministic and lives in the pure ACP adaptation crate. It
renders only the body after the template delimiter. Optional metadata lines are removed as
whole lines. Messages are encoded as a JSON data array with explicit roles inside the
template's untrusted historical-data boundary, so message text cannot close or replace that
boundary. The final UTF-8 payload is at most 15,999 bytes, which also satisfies the settled
limit of fewer than 16,000 characters. It drops oldest whole messages until the payload
fits, never truncates a message, and fails when metadata or even one whole message cannot
fit. Tool calls, tool results, thoughts, permissions and native sessions never enter it.
The template, its historical-data label and its boundary markers form a security boundary;
editing them requires security review, and tests pin the markers relied on by the framing.

The wire grows additively and remains protocol version 1. Switch events identify the pane,
old harness and new harness. The completion result identifies the new session. Failure is a
typed result and states whether the original session was restored.

Harness discovery remains complete. Presentation ranks ready installed entries first,
installed entries needing setup second, and unavailable entries last. Stable display-name
ordering breaks ties. Search filters the ranked view by a case-insensitive name substring;
it does not change availability or remove catalogue entries from the complete list. The
composer uses a searchable grouped control instead of a width-limited strip, and every
catalogue entry remains selectable regardless of window width. A new pane defaults to the
first ready installed entry, then the first installed entry needing setup. No unavailable
entry becomes the implicit default.

Task dispatch that explicitly names a harness uses the same switch transaction when it
targets an existing live pane. It does not create a second switching path.

## Consequences

The visible transcript is honest about the native restart while remaining attached to the
same pane. A failed candidate cannot destroy a usable conversation. Continuation text is
bounded, source-preserving historical data. The catalogue stays exhaustive while putting a
person's usable tools before unfamiliar unavailable entries.
