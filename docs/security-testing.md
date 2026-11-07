# Security testing

Soyeht keeps attacking its own code with tests that run in the merge gate. A static audit finds
what is wrong today; these tests keep finding what a later change breaks. There are two kinds:
fuzz targets (this page) and attack tests against a running engine.

- **Fuzz targets** feed bytes and values that a peer chose to the code that reads them, and
  assert what must hold whatever the input: no panic, no hang, a limit that is kept, an answer
  that is well formed, a check that a mutated input cannot pass.
- **Attack tests** start a real engine and supervisor with the integration rig and try each
  attack, asserting the exact refusal and that nothing changed on disk.

## Fuzz targets

The targets are deterministic, seeded property tests, so they run inside `cargo test` with no
extra tool. (`cargo-fuzz` needs a nightly compiler and a sanitizer runtime, which the pinned
toolchain and the gate do not carry.) The harness is
`engine/soyeht-protocol/tests/fuzz_support/mod.rs`; each crate that has a target includes it with
`#[path]`. It mutates a corpus of valid seeds (bit flips, edge bytes, token splices, truncation,
integer boundaries, JSON-aware field replacement), runs the target under `catch_unwind`, ends the
process when one case runs longer than 20 seconds, and on a failure prints the seed and the
minimized input as hex.

| Target (test file) | What is attacked | What must hold |
| --- | --- | --- |
| `soyeht-protocol/tests/fuzz.rs` | frames, NDJSON, greeting and `hello`, request parameters of every method, attachments, snapshot pages, pairing links, proofs and keys, the signed update manifest | no panic; a length over the limit is refused before any payload is read; reading does not depend on how bytes arrive; accepted parameters are accepted again after the typed model writes them back; a link that parses names only a loopback or tailnet address; a small-order key is never accepted; a changed manifest is never accepted as another offer |
| `soyeht-ptyd/tests/fuzz.rs` | the supervisor's request decoder, and a real supervisor sent framed garbage before and after `hello` | a request before `hello` is refused as `unauthorized`; every answer is a whole well-formed response; the supervisor survives and serves a good client |
| `soyeht-web/tests/fuzz.rs` | the URL policy of web panes | only `http` and `https`, no credentials, no `file`, `javascript` or `data`; the normalized address is stable |
| `soyeht-platform/tests/fuzz.rs` | canonical paths, atomic and exclusive writers | a write lands exactly, owner-only, with no leftover temporary file, never through a link and never over an existing entry |
| `soyeht-engine` `editor::containment_fuzz` (unit tests) | the editor's file resolution and creation, and the agents' text access | nothing outside the workspace is read through a link, written, created or replaced |

Terminal output parsing is fuzzed in the `vt100` fork and in SwiftTerm, not here.

### The short run (the gate)

`cargo test -p soyeht-protocol -p soyeht-platform -p soyeht-web -p soyeht-ptyd --test fuzz` and
`cargo test -p soyeht-engine --lib containment_fuzz` run a fixed number of cases from a fixed
seed per target, so a run is the same every time and takes seconds. This is the gate step
`security:fuzz`; the full engine test run includes the same tests.

### The long run

`SOYEHT_FUZZ_SECONDS=N` makes each target run for N seconds from a fresh random seed (the seed
is printed; `SOYEHT_FUZZ_SEED=<number>` replays one). Use release builds and a low priority:

```sh
cd engine
SOYEHT_FUZZ_SECONDS=300 nice -n 15 cargo test --release -j 4 \
  -p soyeht-protocol -p soyeht-platform -p soyeht-web -p soyeht-ptyd --test fuzz \
  -- --test-threads 4 --nocapture
SOYEHT_FUZZ_SECONDS=300 nice -n 15 cargo test -j 4 -p soyeht-engine --lib containment_fuzz \
  -- --test-threads 2 --nocapture
```

The full tier runs the same targets with a short budget (`security:fuzz-long`, release builds,
`SOYEHT_FUZZ_LONG_SECONDS` per target, default 30). Run them for minutes or hours before a
release, or after a change to a parser, and keep any crash as a test (below).

### When a target fails

The failure message carries the seed and the minimized input as hex. Replay it:

1. Put the hex in a `#[test]` of the same file that calls the target's function on the decoded
   bytes, so the case stays in the suite for good.
2. Fix the code. If the input is a legitimate case the property wrongly forbids, fix the
   property and say why in a comment.
3. A real vulnerability is described in the fix commit by class and fix, not as a recipe.

### Adding a target

Add a test to the crate's `tests/fuzz.rs` (or create one that includes the harness) with a corpus
of valid seeds and a few tokens of the grammar, and the property. Keep the default budget to
about a second: a target that does synced disk writes uses a few hundred cases at most.

## Attack tests

Each test starts a disposable engine and supervisor (`Rig`), attacks, and asserts the exact
refusal (the error code of the contract) and the state of the disk. They are the
`attack_*.rs` modules of `engine/soyeht-engine/tests/runtime/`, plus a hostile-repository test
in `engine/soyeht-vcs/tests/`.

| Module | Attacks |
| --- | --- |
| `attack_wire` | frames over the limit or of an unknown kind (closed before the payload is read); a frame cut short; JSON that is not a request; every method of the contract before the `hello` (`unauthorized`); a second `hello` (`unauthorized`, connection ended); an unsupported protocol version; an unknown agent binding; every method an agent-bound connection does not hold (`forbidden_role`) |
| `attack_pairing` | a forged proof, a proof for another request or connection, codes never issued, weak or non-canonical device keys, forged and replayed `hello` signatures, unknown tokens, the connection limit of a device, a paired device deciding pairings or revoking others |
| `attack_paths` | `..`, absolute paths, links that lead out and the spellings of other systems through `file.open`, `project.tree.list`, `project.tree.watch`, `project.entry.create`, `project.entry.delete`, `writing.document.create`, `writing.document.open` and an agent's file requests; pipes, link loops and files over the limit |
| `attack_web` | `file`, `javascript`, `data`, `blob`, `about` and other schemes, credentials in an address, through every method that takes an address; the web pane limit of a workspace |
| `attack_process` | hostile task text and a hostile folder name reach `wt` only as whole arguments |
| `attack_floods` | more local and remote connections than the engine admits; a web page reaching for the remote listener; a message past the frame limit; a prompt past the size limit; a request exactly at the frame limit |
| `soyeht-vcs` `hostile_repository` | a repository whose configuration names programs (monitor, filters, text conversion, pager, hooks) starts none of them when the editor reads it; a file named like an option stays a file name |

Run them with
`cargo test -p soyeht-engine --features soyeht-engine/contract-checks --test runtime attack_`
(after `cargo build --workspace --features soyeht-engine/contract-checks`, which builds the
programs the rig starts) and `cargo test -p soyeht-vcs --test hostile_repository`. The gate step
is `security:attacks`.

### Adding an attack

Add a test to the module of its class. Assert the exact error code, and that nothing changed:
compare the files outside the workspace before and after, or the panes and workspaces the
request would have made. A method with a role or path rule is covered by the matrices of
`attack_wire` and `attack_paths` without a new test; add an entry when a new method takes a path.

## The gate steps

`scripts/check_all.sh` is the only list of steps.

| Step | Area and tier | Runs |
| --- | --- | --- |
| `security:fuzz` | `engine`, fast and full | the short fuzz run: the `fuzz` tests of the protocol, platform, web and supervisor crates, and the engine's `containment_fuzz` |
| `security:attacks` | `engine`, fast and full | the `attack_` tests against a running engine, and the hostile-repository test |
| `security:fuzz-long` | `engine`, full only | the same fuzz targets for `SOYEHT_FUZZ_LONG_SECONDS` each (default 30), in release builds |

`security:attacks` needs the programs the rig starts, which `engine:build` builds earlier in the
same run. Both fast steps run whenever the plan selects the engine, so a change to any engine
crate is attacked, not only a change to the crates that have fuzz targets.
