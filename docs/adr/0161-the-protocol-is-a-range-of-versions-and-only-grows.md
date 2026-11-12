# ADR 0161: The protocol is a range of versions, and the contract only grows

- Status: proposed (the range, the negotiation and the check are implemented; open enums wait for the owner)
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
`MIN_PROTOCOL_VERSION`; Swift `ProtocolVersion.current` and `.min`; the same two values in the
TypeScript output. Nobody types either number.

### Negotiation

`hello.protocolVersion` keeps its meaning: the highest version the client speaks. No new `hello`
field.

- The engine refuses a `hello` below `min` with `protocol_version_unsupported` (no new error code).
  The message tells the person which side to update ("update Soyeht on this device").
- Otherwise it welcomes at `min(client version, current)`. `welcome.protocolVersion` is the
  negotiated version, and the engine keeps it on the connection. The welcome also carries an
  optional `minProtocolVersion` (additive), so a client can say what to update.
- The client accepts a welcome whose version is in its own `[min, current]` and refuses anything
  else (`UnexpectedHandshake` in Rust, `invalidFrame` today in Swift, which should say "update
  Soyeht on the Mac"). A phone newer than its Mac therefore works when the Mac's version is at or
  above the phone's `min`; the phone, not the engine, decides that.
- Equality checks go away. The three client checks share one rule, `handshake::accepts_welcome`
  (Rust) and its Swift counterpart over one vector file in `contracts/vectors/`.

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

1. Engine-to-client enums that can grow are made open. A count showed the cost is large: of the 95
   enums of the contract, 77 are reachable from a result or an event (19 of them from params as
   well), 15 only from params, and 3 from neither. Converting 77 means an `unknown(String)` case in
   each generated Swift enum and a catch-all in Rust, and every exhaustive `switch` of the Mac and
   iOS views changes. That is above the threshold the owner set, so nothing is converted yet. Until
   the owner decides, the check treats every engine-to-client enum as closed: a new value is a
   break (and a new `ErrorKind` is one too). Converting one is a data change: name it in the
   top-level `x-openEnums` list of `protocol.json`, which the check already reads.
2. A client below `min` is refused by the engine; a client that is welcomed at a version outside
   its range shows the update message at connect time (`protocol_version_unsupported`). The updater
   is not blocked.
3. Any additive change after a release raises `current`; the check fails until it is raised.
4. A break is a deliberate `min` bump with a `minReason`.

## Not decided here

- Which engine-to-client enums become open (item 1 above).
