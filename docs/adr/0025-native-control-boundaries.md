# ADR 0025 · Bound native controls and preserve uncertain operations

Status: accepted for slice 2.

Native metadata is not an unlimited public payload. Authentication IDs and
session IDs fit the public 128-character identifier bound. An initialization
accepts at most 32 authentication methods and 256 KiB of their serialized
metadata. Names are limited to 256 characters, descriptions to 4096, and each
method's arguments and environment to 64 KiB combined. Argument and environment
counts are each limited to 64; individual values are limited to 16 KiB. Reject
invalid names, NUL bytes and duplicate authentication IDs. Reported versions
are bounded at 256 characters. The supervisor still applies its environment
filter; native metadata does not grant extra inherited credentials.

A successful configuration probe must contain the requested confirmed option.
A success-shaped but incomplete response is inconclusive. Permission modes are
the harness's own and are never mapped to a Soyeht policy (ADR 0070, which
replaced the Yolo override described here before). Private history replay does not publish configuration-history
notifications before the replay is committed.

The local supervisor protocol is version 4. `Kill` terminates a process group
while preserving retained output; `Close` releases it. A PTY spawn that fails
before any process survives returns `spawn_failed`. The engine settles that
native terminal request with an error and frees its reservation. A transport or
storage failure remains ambiguous and retains the original spawn key for
reconciliation; it must not be reported as proof that nothing started.

Disconnecting a client removes only its transient native-operation waiters.
The persisted operation and idempotency receipt remain available to a later
connection. A missing client cannot consume the waiter limit permanently.

A configuration call without confirmation for 90 seconds returns an explicit
failure. The native request and operation remain recorded, blocking conflicting
configuration and queued prompts until a late response or process loss resolves
them. A late confirmation updates the actual settings; retrying the original
idempotency key keeps its timeout result. No timeout restarts the session or
resends a setting whose application is uncertain.
