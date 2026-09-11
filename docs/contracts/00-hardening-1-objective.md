# Objective · hardening round 1 (before slice 3)

Decided by Caio on 2026-09-11 after the architecture audit
(`docs/reports/audit-2026-09-11.md`) and the slice 2 review
(`docs/reports/slice-2-review.md`). Executed autonomously, one subject per
PR, local gate with the real restricted list before every merge, gate output
pasted in the PR. Starts only after the ten slice 2 follow-ups are on `main`.

## Problem
The engine's business rules are welded to concrete IO and to failure by
panic. Measured on `main` a0c860a: raw SQL in 11 engine files (15 import
the database crate); 197 `unwrap`/`expect` in engine `src`, 127 outside
test modules; zero traits in the workspace; two NDJSON decoders with
different behaviour on oversized lines; one 263-line function in the process
supervisor; Dev profile constants typed in four languages; event names typed
by hand in the Swift client. Every slice makes these numbers worse.

## Expected outcome
1. **No SQL outside `soyeht-store`.** The engine does not depend on the
   database crate. Every query the engine needs is a named function in the
   store with its own test on a real database.
2. **Rules behind ports.** The engine's rules (workspaces, panes, agent
   sessions, pairing, input) call the store, the supervisor client and the
   clock through traits defined in the engine, with the real implementation
   and a test double. At least one rule test per module runs without a
   database, a socket or a process.
3. **No panic in production paths.** `clippy::unwrap_used`,
   `clippy::expect_used` and `clippy::panic` are `deny` for every crate's
   non-test code; the remaining uses are converted to typed errors. Test
   modules keep them.
4. **One error enum per crate**, named variants, and one mapping to the wire
   `ErrorKind` in the engine. No `Error(String)`, no `io::Error::other("text")`
   for domain failures.
5. **One frame reader and one NDJSON decoder** in `soyeht-protocol`, used by
   the CLI, the supervisor client and the ACP client, with one test for the
   oversized-line case that both former decoders now share.
6. **Supervisor `execute` split** into one function per request kind; no
   function in the workspace above 120 lines outside generated code.
7. **Dev profile from one source.** A `contracts/profiles.json` (or an
   `x-profiles` block in `protocol.json`) generated into Rust, Swift and the
   Python scripts: data directory, socket names, port, launchd labels.
8. **Events generated.** `contracts/generate.py` emits an event registry for
   Swift, TypeScript and Rust; the Swift client dispatches on it; renaming an
   event without regenerating fails to compile. `ModelListResult.source`
   becomes a closed enum. `engine_identity_changed` is added to `x-errorCodes`
   or mapped to an existing kind.
9. **Swift terminal host deduplicated**: one shared host in `SoyehtClient`,
   two thin platform wrappers.
10. **Docs corrected in the same PRs**: `docs/reports/slice-1.md` counts,
    `README.md` mentions of directories that do not exist yet (only if Caio
    approves the README wording), ADR headings 0022–0025 to the common format.

## Must not change
- The wire protocol: no method, event, field or error code changes except
  the two named in item 8; `contracts/generate.py --check` stays green.
- The ten slice 2 acceptances and the six slice 1 acceptances keep passing
  unchanged; no test is deleted, ignored or weakened.
- The Dev profile stays compatible with its current database (schema
  version bumps only with a migration and a migration test).
- `README.md` product text and `design/soyeht-desktop.pen` are not touched.

## Gate additions (ratchets, in `scripts/check_all.sh`)
- `check-no-sql-outside-store`: fails if `rusqlite` appears in any
  `Cargo.toml` except the store's, or if SQL keywords appear in engine `src`.
- `check-no-panic-in-production`: the three clippy lints above as `deny`.
- `check-verification-not-weakened`: counts tests, acceptances and gate
  steps on the PR base and on the PR head; a drop fails unless the PR body
  has a `Verification change:` line with the reason.
- `check-function-length`: warns above 120 lines, fails above 200, outside
  generated code.
- `check-function-length` and `check-no-panic-in-production` are ratchets:
  the counts recorded on `main` may only go down.

## Out of scope
New features, new methods or events, iPhone changes beyond item 9, the
browser and editor slices, any change to contracts 01–11 other than the
docs corrections in item 10.

## Acceptance
- All ten items above true on `main`, each with the file and line in the
  closing report `docs/reports/hardening-1.md`.
- The measurements of the "Problem" section re-taken and recorded: raw SQL
  files 0, panics outside tests 0, traits ≥ 3 (store, supervisor, clock),
  decoders 1, longest function ≤ 120, profile sources 1.
- Full local gate green with the new ratchets, output pasted in the report
  with date and `main` hash.
