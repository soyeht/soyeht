# ADR 0161: The protocol is a range of versions, and the contract only grows

- Status: proposed (implemented on its branch; the owner has not accepted it yet)
- Date: 2026-11-11

## Context

The protocol version is the literal `1`: `x-protocolVersion` in `contracts/protocol.json`, a constant
in each generated output, and an equality check in four places (the engine's `hello` in
`dispatch.rs`, `handshake::expect_welcome` and `expect_device_welcome` for the Rust clients, and
`EngineClient.swift` for the Swift ones). The engine's own `hello` check is the bare literal `1`, not
the generated constant. After launch, a phone and a computer update at different times, so any
protocol change would lock out whichever side updated last. Contract 08 §1.6 already says "the
engine accepts current and previous versions"; the code does not do that.

## Decision

### A range, in the contract

`x-protocolVersion` becomes `{"min": 1, "current": 1}`. `current` is the version this build speaks;
`min` is the oldest version it still serves. Generated: Rust `PROTOCOL_VERSION` (= `current`) and
`MIN_PROTOCOL_VERSION`; Swift `ProtocolVersion.current` and `.min`. (The TypeScript output carries
the range only inside `schema.json`.) Nobody types either number.

### Negotiation

`hello.protocolVersion` keeps its meaning: the highest version the client speaks. No new `hello`
field.

- The engine refuses a `hello` below `min` with `protocol_version_unsupported` (no new error code).
  The message is one text of `x-clientPolicy` (`texts.updateSoyeht`): "update Soyeht on the older
  one".
- Otherwise it welcomes at `min(client version, current)`. `welcome.protocolVersion` is the
  negotiated version; the engine only echoes it in the welcome and keeps nothing on the connection
  (nothing branches on it yet; the day something does, the connection records it).
- The client accepts a welcome whose version is in its own `[min, current]` and refuses anything
  else (`Error::UnsupportedProtocolVersion` in Rust, `ClientFailure.unsupportedProtocolVersion` in
  Swift; both say the same `updateSoyeht` text). A phone newer than its Mac therefore works when the
  Mac's version is at or above the phone's `min`; the phone, not the engine, decides that.
- Equality checks go away. The rule is `handshake::negotiate_in` and `accepts_in` (Rust, with
  `negotiate` and the client check as wrappers for this build's range) and
  `ProtocolVersion.accepts` (Swift), over one vector file in `contracts/vectors/`.

The negotiated version selects no wire shape: the contract below is additive, so one set of shapes
serves every version in the range. It is for the engine to withhold something a client at an older
version would mishandle (an unsolicited event, for example), never for a branch per version.

### Additive-only

From the first release, a change to the wire surface (methods, events, `$defs`, error codes, frame
kinds, limits) may only add. "Add" means exactly:

| Allowed | Forbidden |
|---|---|
| a new method, event, `$def` or error code | removing or renaming any of them |
| a new optional property | removing or renaming a property; changing its type, format or bounds |
| a new required property of a type the engine sends (result, event) | a new required property of a type the client sends (params); making an optional property required |
| a new value of an enum a client sends | a new value of an enum the engine sends, unless the enum is declared open (below) |
| loosening a bound on params, tightening one on results | the reverse; changing a code's number or meaning; a limit that shrinks |

Direction comes from reachability: a type reachable from a method's params is client-to-engine, one
reachable from a result or event is engine-to-client, and a type that is both gets the stricter
column. Readers already ignore unknown fields and unknown events (contract 08 §1.5); the engine must
not send an unsolicited event that old clients cannot ignore.

A new value of an enum the engine sends (including a new `ErrorKind`) breaks a client whose generated
decoder has a closed enum. So an engine-to-client enum that may grow is marked open in the contract,
and the generated Swift decodes an unknown string to an `unknown(String)` case. Phase 2 counts the
closed enums that are sent by the engine and decides which must be open; a closed one stays frozen.

What the check cannot see (the meaning of an existing field, the effect of an existing method) is
the reviewer's: a PR that touches `protocol.json` after launch states in its body that existing
behaviour is unchanged. That line is part of the review checklist, not a gate step.

### How `generate.py --check` enforces it

The last released contract lives in `contracts/released/`: `protocol-v<N>.json`, the wire surface of
the last release whose `current` was N, one file per version in `[min, current]`, plus `index.json`
mapping `N` to the `productVersion` of `contracts/release.json` it shipped in. The files are written
by `scripts/release.py snapshot`, which `release_flow.py prepare` runs (it writes the file for the
version being released and never overwrites one), and `release_flow.py build` refuses a release
whose `current` has no snapshot or whose snapshot differs from `protocol.json`. A pure checker
(`contracts/compat.py`, tested with a table of allowed and forbidden diffs) compares `protocol.json`
to every snapshot with `N >= min` and prints one line per violation. `generate.py --check` runs it,
so it is a gate step already (`contracts:generate-check`). Two more rules in the same check:

- if `protocol.json`'s wire surface differs from `protocol-v<current>.json` (when that file exists),
  `current` must be raised: an additive change after a release is a new version;
- a build with no snapshot at all (before the first release) is not checked; the first release
  writes `protocol-v1.json`, from which the rule applies.

### A breaking change

A break is deliberate and rare: raise `min` above the last version that still needs the old shape,
delete the snapshots below the new `min` in the same PR, and write `x-protocolVersion.minReason`
(required, non-empty whenever `min > 1`; `release.py check` and the compat check both read it). The
reason names what is removed and why an addition could not do. Clients below the new `min` are
refused with `protocol_version_unsupported` and have to update; the release notes say so. The
changelog entry of the release is the place the owner reads it before publishing.

## Consequences

- An old phone keeps working with a new computer and the reverse, within `[min, current]`.
- The Rust client, the CLI, the MCP server and the Android core all go through
  `soyeht_protocol::handshake`, so one change covers them; the Swift client has its own copy of the
  rule, kept equal by a shared vector.
- The attack test that refused every other version is two tests now: a version above `current` is
  welcomed at `current`; only 0 (below `min`) is refused.
- A new engine must keep serving old shapes for ever, which is the cost of the rule. The deliberate
  `min` bump is the way out.

## Defaults taken, each easy to change

1. Engine-to-client enums that can realistically gain values after launch are open, and the rest
   stay closed. Of the 95 enums of the contract, 77 are reachable from a result or an event; 19 of
   those are open (the `x-openEnums` list of `protocol.json`): `ErrorKind`, the tool, prompt,
   transcript and turn-stop vocabularies (`ToolStatus`, `PromptStatus`, `TranscriptKind`,
   `TurnStopReason`), `PermissionOptionKind` and `PermissionRequestKind`, the harness setup states
   and reasons (`HarnessSetupState`, `HarnessSetupReason`, `HarnessEnvironmentIssue`,
   `AgentLoginPhase`), the usage states, sources and reasons (`UsageState`, `UsageSource`,
   `UsageUnavailableReason`), and `LspState`, `ReadinessSystem`, `SubagentStatus`,
   `TaskReviewState` and `EngineReachabilityReason`. A client keeps a value it does not know
   (Swift `unknown(String)`, which keeps the text and writes it back; Rust `Unknown`, a unit
   variant) and shows it as a generic state or as the text it carries.
   The Rust variant exists only with the `open-enums` feature of `soyeht-protocol`, which only the
   Windows and Linux client and the Android core turn on (their cargo workspaces). The engine, the
   CLI and the MCP server are built without it, so for them `Unknown` is not a value of the type:
   wherever the engine reads one of these enums (a request, an agent's update or permission
   options, its stop reason, a stored row) a value nobody defined is refused exactly as before, and
   it cannot publish one. The variant is also never written out (`skip_serializing`), so a client
   cannot send it back. A test per read site of the engine and one over every open enum prove it.
   `PaneState` and `TaskCheckState` already have a value called `unknown`, which the catch-all
   would clash with, so they stay closed. An open enum cannot be closed again (the check refuses
   it). The rest are closed: a new value of one of them is a break, which a deliberate `min` bump
   makes.
2. A client below `min` is refused by the engine; a client that is welcomed at a version outside
   its range shows the update message at connect time (`protocol_version_unsupported`). The updater
   is not blocked.
3. Any additive change after a release raises `current`; the check fails until it is raised.
4. A break is a deliberate `min` bump with a `minReason`.

## Not decided here

- Whether `PaneMode`, `TaskState`, `UsageProvider` and `EditorLanguage` become open. The engine can
  now refuse an unknown value in a request (it has no `Unknown`), so the only cost left is the
  `unknown` case in the Mac and iPhone views that switch over them.
