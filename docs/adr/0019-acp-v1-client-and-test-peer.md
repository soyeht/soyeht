# ADR 0019 · ACP v1 client and deterministic test peer

Status: accepted. Scope: the client library and test peer in slice 2.

## Boundary

`soyeht-acp` adapts ACP v1 messages and options. It does not own processes,
write application state or decide whether a user permission wins. Those
belong to the engine and supervisor. Its source is the
[pinned official v1 schema](https://github.com/agentclientprotocol/agent-client-protocol/blob/2821a8b51dca2aeb3d48b73c9214a6c79968c975/schema/v1/schema.json).
Version 2 is refused. The prompt response's `stopReason` ends a v1 turn.

The client advertises filesystem, terminal, terminal authentication, form
elicitation and boolean session-configuration capabilities. The engine
integration must implement those methods before using this initialization.
URL elicitation is not advertised. Unknown session-update variants are
ignored without reinterpreting them as a known kind. Known updates must
carry the owning session ID.

## Correlation and configuration

The request allocator and outstanding calls are serializable. The engine
persists them with its outgoing operation before writing native bytes,
and restores them when adopting the existing process. The line decoder
tracks exact byte intervals and acknowledges only complete JSON records;
fragmented UTF-8 cannot change the offset. Native string, integer and null
request IDs are distinct; an absent ID means notification.

Select and boolean options retain native IDs. Group labels are included
in the displayed choice name without changing its value. Missing categories
remain valid. Legacy `modes` use `session/set_mode`; a native option takes
precedence over that compatibility path. Boolean changes include the
native `type: "boolean"` discriminator. No selection becomes confirmed
just because it was sent: the reply must contain the requested value.
A full config reply preserves a separately advertised legacy mode.

Configuration metadata is bounded to 64 selectors, 256 choices per selector
and 512 KiB after normalization. Native option and choice identifiers must
fit the public protocol's nonempty, 128-character ID bound. Exceeding a limit returns an explicit
adapter error rather than truncating a catalog or growing a snapshot
without bounds. These are implementation limits, not ACP guarantees.

A fingerprint covers executable digest, reported version and launch
configuration. A successful probe means `worked`; method-not-found means
`unsupported`; authentication-required means `unauthenticated`; other
errors and timeouts are `inconclusive`. An advertised capability is only
an input to probing. A parameter error is not proof of lost support.
Terminal authentication runs the configured program with additional
arguments and filtered environment; it is never sent to `authenticate`.

## Deterministic peer

`soyeht-fake-acp` requires an absolute `--state-dir` pointing to a disposable
test directory. Its entry file is named `program.rs` so Cargo does not also
discover it as an integration test; ordinary integration-test discovery stays
active. The Dev installer does not install this test binary.

The peer persists its own sessions and configuration, emits message chunks,
requests permission or a form, changes configuration, supports cancellation,
replays a loaded transcript, requests filesystem/terminal operations and can
exit deliberately. Login uses a separate invocation. Load support can be
truthful, absent, falsely advertised, missing a session or unresponsive.
All of this uses local data and deterministic text, without a model service.

Tests exercise the peer as a real subprocess: prompt chunks, confirmed model
and effort, rejected parameters without a session restart, null-ID permission,
elicitation after allow-all, cancellation without process death, resume after
process death, false capability advertising and terminal login. Engine-level
recovery and client delivery are the next contracted implementation step.
