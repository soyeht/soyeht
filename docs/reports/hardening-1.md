# Hardening round 1 · delivery and verification

Date: 2026-09-11. Implementation `main` validated at
`be9c927d95bb7d785b04b30045fd4fa093d7c2d5`.

The round is complete. All ten outcomes in
`docs/contracts/00-hardening-1-objective.md` are on `main`. The local merge
gate passed after the last outcome with the real restricted vocabulary. No
method or event changed. The only protocol changes were the two changes allowed
by the objective. No test, acceptance or gate step was removed.

## Delivered outcomes

### 1. Persistence stays in the store

The engine depends on `soyeht-store` and has no database-driver dependency
(`engine/soyeht-engine/Cargo.toml:9-26`). Named persistence operations are
owned by the store (`engine/soyeht-store/src/domain.rs:1-7,67-90`). Four test
groups open a real temporary store and cover workspace, pane, settings,
input, idempotency, pairing and agent cleanup operations
(`engine/soyeht-store/tests/domain.rs:18-26,82-264`).

The gate scans every non-store manifest and every non-generated Rust source
outside the store for database statements
(`scripts/hardening_gate.py:315-340`). Its final count is zero files and zero
violations. Delivered in [PR 54](https://github.com/soyeht/soyeht/pull/54).

### 2. Engine rules use explicit IO ports

The engine owns exactly three effect boundaries: `Clock`, `SupervisorPort`
and `StorePort` (`engine/soyeht-engine/src/ports.rs:23-35,46-64,79-105`). The
same module supplies `FixedClock`, `FakeSupervisor` and `MemoryStore` for rule
tests (`engine/soyeht-engine/src/ports.rs:318-369`).

Workspace, pane, agent-session, pairing and input rules run against those
doubles without opening a database, socket or process
(`engine/soyeht-engine/src/workspaces.rs:124-144`,
`engine/soyeht-engine/src/panes.rs:302-324`,
`engine/soyeht-engine/src/agents/driver.rs:528-559`,
`engine/soyeht-engine/src/pairing.rs:301-333`,
`engine/soyeht-engine/src/input.rs:110-172`). The transaction boundary remains
inside the store port. The decision is recorded in
`docs/adr/0028-engine-io-ports.md:1-22`. Delivered in
[PR 56](https://github.com/soyeht/soyeht/pull/56).

### 3. Production Rust cannot panic through convenience calls

The workspace denies `unwrap_used`, `expect_used` and `panic`
(`engine/Cargo.toml:28-35`). All six crates opt in through their `[lints]`
sections (`engine/soyeht-acp/Cargo.toml:22-23`,
`engine/soyeht-cli/Cargo.toml:19-20`,
`engine/soyeht-engine/Cargo.toml:31-32`,
`engine/soyeht-protocol/Cargo.toml:14-15`,
`engine/soyeht-ptyd/Cargo.toml:17-18`,
`engine/soyeht-store/Cargo.toml:22-23`). Test-only code keeps explicit local
allowances. The production scan reports zero calls. Delivered in
[PR 57](https://github.com/soyeht/soyeht/pull/57).

### 4. Errors are typed at crate boundaries

Each Rust crate owns one named error enum:
`engine/soyeht-protocol/src/error.rs:4`,
`engine/soyeht-store/src/database.rs:12`,
`engine/soyeht-ptyd/src/error.rs:4`,
`engine/soyeht-acp/src/rpc.rs:14`,
`engine/soyeht-engine/src/errors.rs:7` and
`engine/soyeht-cli/src/error.rs:5`. No production enum has an unnamed
`Error(String)` variant, and domain failures do not use
`io::Error::other`.

The engine has one translation from its internal failures to the public
`ErrorKind`, including store and supervisor helpers
(`engine/soyeht-engine/src/errors.rs:123-225`). Delivered in
[PR 58](https://github.com/soyeht/soyeht/pull/58).

### 5. Stream decoding has one implementation

The protocol crate owns the binary frame reader
(`engine/soyeht-protocol/src/frame.rs:21-50`) and the sole bounded NDJSON
decoder (`engine/soyeht-protocol/src/ndjson.rs:19-90`). The CLI and supervisor
use the shared frame reader (`engine/soyeht-cli/src/lib.rs:10,53`,
`engine/soyeht-ptyd/src/server.rs:6,115,170`). The supervisor's pending-request
ledger and the ACP client use the shared NDJSON decoder
(`engine/soyeht-ptyd/src/pending.rs:1-24`,
`engine/soyeht-acp/src/rpc.rs:274-284`).

One protocol test fixes the converged oversized-line behavior: discard the
oversized record with bounded memory, then decode the next exact record
(`engine/soyeht-protocol/src/ndjson.rs:93-121`). Delivered in
[PR 61](https://github.com/soyeht/soyeht/pull/61). Supporting deterministic
test corrections landed in [PR 59](https://github.com/soyeht/soyeht/pull/59)
and [PR 60](https://github.com/soyeht/soyeht/pull/60).

### 6. The supervisor dispatches through bounded functions

`execute` is now a small request dispatcher
(`engine/soyeht-ptyd/src/server.rs:189-220`). Each request kind has its own
function beginning at `engine/soyeht-ptyd/src/server.rs:223-472`.

The gate measures Rust, Swift, Python and TypeScript functions while excluding
generated sources (`scripts/hardening_gate.py:315-357,415-446`). No function
is above 120 lines. The longest production function is `advance_agent`, exactly
120 lines (`engine/soyeht-engine/src/agents/driver.rs:403-522`). Delivered in
[PR 62](https://github.com/soyeht/soyeht/pull/62).

### 7. The Dev profile has one source

`contracts/profiles.json` is the only source for the Dev data directory,
socket names, TCP port and service labels (`contracts/profiles.json:1-17`). The
generator validates that source and renders Rust, Swift, Python and both
service files (`contracts/generate.py:288-335,336-412`). Generated outputs
carry the same source digest, for example
`engine/soyeht-protocol/src/profile.rs:1-7`,
`contracts/swift/Sources/SoyehtProtocol/Profile.swift:1-10` and
`scripts/generated_profile.py:1-10`. Delivered in
[PR 63](https://github.com/soyeht/soyeht/pull/63).

### 8. Protocol events are generated and closed

The generator emits registries from every `x-events` entry for Rust, Swift and
TypeScript (`contracts/generate.py:140-198`). The outputs include the Rust
`EventName` and `EVENTS` table
(`engine/soyeht-protocol/src/generated.rs:2363-2495`), the Swift exhaustive
enum and registry (`contracts/swift/Sources/SoyehtProtocol/Generated.swift:2402-2453`),
and the TypeScript keyed registry
(`contracts/typescript/src/generated.ts:1292-1337`).

The shared Swift client converts the wire name to the generated enum and uses
an exhaustive switch (`apps/shared/Sources/SoyehtClient/EngineClient.swift:243-314`).
The generator also requires `ModelListResult.source` to reference a closed
`ModelListSource` enum (`contracts/generate.py:268-285`). The only wire change
in this outcome is the allowed `engine_identity_changed` error at
`contracts/protocol.json:98,4990`; no method or event changed. Delivered in
[PR 64](https://github.com/soyeht/soyeht/pull/64).

### 9. Apple clients share one terminal host

`TerminalHost` owns stream attachment, replay-safe feeding, selection and
resize synchronization, input forwarding and terminal delegate behavior
(`apps/shared/Sources/SoyehtClient/TerminalHost.swift:4-70`). The macOS wrapper
keeps focus, appearance and writable input policy
(`apps/shared/Sources/SoyehtClient/MacTerminal.swift:6-44`). The iOS wrapper
keeps appearance and read-only input policy
(`apps/shared/Sources/SoyehtClient/PhoneTerminal.swift:6-35`). Delivered in
[PR 65](https://github.com/soyeht/soyeht/pull/65).

### 10. Documentation matches the implemented surface

The slice 1 report now distinguishes its original 26-method, 22-event surface
from the current 40-method, 39-event schema
(`docs/reports/slice-1.md:11-17`). ADRs 0022 through 0025 already use the
canonical `# ADR NNNN · Title` heading
(`docs/adr/0022-native-history-recovery.md:1`,
`docs/adr/0023-agent-output-loss-and-deadlines.md:1`,
`docs/adr/0024-interruption-pauses-the-agent-queue.md:1`,
`docs/adr/0025-native-control-boundaries.md:1`). Their normalization landed in
[PR 49](https://github.com/soyeht/soyeht/pull/49); the count correction landed
in [PR 66](https://github.com/soyeht/soyeht/pull/66).

The objective made the root README correction conditional on approved wording.
No wording was approved for this round, and the README and design file were
left unchanged.

## Measurements

The same scanner used by the merge gate measured the working tree at the
validated `main` hash. Generated files are excluded by their generated header,
and test-only Rust regions are excluded from the panic count
(`scripts/hardening_gate.py:315-357`).

| Measurement | Before | After |
| --- | ---: | ---: |
| Engine files with raw SQL | 11 | 0 |
| Panic-capable calls in production Rust | 127 | 0 |
| Engine IO port traits | 0 | 3 |
| NDJSON decoder implementations | 2 | 1 |
| Longest production function | 263 lines | 120 lines |
| Canonical Dev profile sources | 4 copies | 1 source |

The three ports are the declarations at
`engine/soyeht-engine/src/ports.rs:23,46,83`. The one decoder is the `Decoder`
at `engine/soyeht-protocol/src/ndjson.rs:24`. The one profile source is
`contracts/profiles.json:1`; its SHA-256 at validation was
`ae5ea04d7e6ef837f17dea31727a46d2edd4822ff825134a06c6482dc17cc15f`.

## Final local gate

The merge script invokes the restricted-vocabulary scanner and all four
hardening ratchets before generation, builds, tests and acceptances
(`scripts/check_all.sh:6-28`). The ratchets landed first in
[PR 51](https://github.com/soyeht/soyeht/pull/51). Hosted workflows remained
disabled; `docs/dev-local.md:3-9` defines the local script as the merge gate.

The full gate ran on 2026-09-11 against implementation `main`
`be9c927d95bb7d785b04b30045fd4fa093d7c2d5`. The restricted vocabulary was
provided only through the environment. The complete local log remained under
the ignored `.cache` directory. Condensed evidence from that log and its JSON
reports was:

```text
forbidden-terms: clean
check-no-sql-outside-store: files base=0 head=0 violations base=0 head=0
check-no-sql-outside-store: passed
check-no-panic-in-production: base=0 head=0
check-no-panic-in-production: passed
check-verification-not-weakened: tests base=151 head=151
check-verification-not-weakened: acceptances base=20 head=20
check-verification-not-weakened: gate steps base=25 head=25
check-verification-not-weakened: passed
check-function-length >120: base=0 head=0
check-function-length >200: base=0 head=0
check-function-length: max base=120 head=120
check-function-length: passed
e2e.json: passed=True; acceptances=6/6; seconds=8.06
e2e-agents.json: passed=True; acceptances=10/10; seconds=7.2
Swift: 352 typed message vectors passed
Swift: 3 binary vectors and rejection controls passed
Swift client tests: 10 passed
macOS and iOS builds: 2 BUILD SUCCEEDED
TypeScript: 359 passed; 0 failed
check_all: passed
```

This run covered protocol generation without a diff, generator and gate tests,
Rust formatting, Clippy with warnings as errors, all Rust builds and tests,
the six shell and ten agent acceptances, shared Swift tests and formatting,
unsigned macOS and iOS builds, and TypeScript checks and tests.

## Additional correction and remaining limits

During physical-device acceptance, a response from a second client exposed a
subscriber close and stale client state. The engine now records every close
reason, and the shared client reconnects through snapshot and cursor-based
subscription. The exact two-client path was reproduced and re-verified
(`docs/reports/acceptance-11.md:35-58`). The correction landed in
[PR 53](https://github.com/soyeht/soyeht/pull/53).

The optional real-adapter compatibility test was not repeated by this final
gate. Physical-device acceptance had already passed and remains recorded in
`docs/reports/acceptance-11.md:17-33`. Four client presentation issues remain
outside this hardening objective: the sign-in card condition, background
refresh, raw permission detail and repeated usage rows
(`docs/reports/acceptance-11.md:79-84`).

## Decisions

- `docs/adr/0027-hardening-ratchet-adoption.md:1-24` records the temporary
  ratchet used to land the gate before removing inherited debt. Each final
  debt count is now zero, so recurrence fails closed.
- `docs/adr/0028-engine-io-ports.md:1-22` records the three engine-owned IO
  ports and keeps transaction ownership in the store.

No tag or release was created. Every change entered through a pull request,
passed the local gate with the real restricted vocabulary and was reviewed
after merge.
