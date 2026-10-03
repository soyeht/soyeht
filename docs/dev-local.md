# Run the new Dev profile

GitHub Actions is disabled until the product is ready. Before every merge, run
`bash scripts/check_all.sh` with `FORBIDDEN_TERMS` supplied through the environment.
`scripts/check_all.sh` is the only list of gate steps; it runs them in sequence
and stops at the first failure. Record the results in the PR description. To
also scan PR history and the final title/body, pass
`--base FULL_BASE_OID --event-file PATH_TO_EVENT_JSON`. Without `--base`, the
ratchets compare with the merge-base of `origin/main` and `HEAD`, and fail when
it cannot be resolved; run `git fetch origin main` first. Keep the vocabulary
outside the repository. `.github/workflows/check-all.yml` only calls
`check_all.sh`, for when hosted CI resumes; add new steps to the script, never to
a workflow.

The editor performance proof is collected separately on VM1, from the same
checkout as the PR. Set `SOYEHT_EDITOR_METER_VM=1` and a locally configured
`SOYEHT_DEVELOPMENT_TEAM` in the VM (never commit either value), then run
`uv run python scripts/check_editor_performance.py --measure`. The command builds
and UI-tests a signed Release app three times; it does not run on the host.
The measurement app has its own bundle ID and does not replace a running Dev app.
Copy `.cache/editor-performance-vm.json` to the review checkout's `.cache`
before `check_all.sh`. Configure the VM's SSH address outside the repository
(for example, `SOYEHT_VM_SSH`), never in a script or report. The local gate
rejects a missing/Debug/slow report or one whose editor Mac, SoyehtClient,
protocol, or meter files changed. An engine-only commit can reuse a still-valid
report. Key and ordinary-scroll work each require median p95 < 8 ms across the
three runs; every run asserts real-screen text ink, aligned gutter, 240 typed
characters, movement, and at least 20 page changes. Page-load timing remains
diagnostic under #404; the old CPU bitmap stopwatch is gone.
The report prints the sample count for every p95. With 24 samples, p95 is the
second-highest observed value, not a strong population estimate; the gate uses
the median of three Release runs.

Install the tools listed in `docs/toolchains.md`. Build from the repository root:

```sh
cargo build --manifest-path engine/Cargo.toml --locked --workspace
uv run python scripts/install_dev.py
engine/target/debug/soyeht --profile dev status
engine/target/debug/soyeht --profile dev pane create --shell
engine/target/debug/soyeht --profile dev pane list
```

The installer signs every profile binary with the machine's Apple Development
identity and a stable `com.soyeht.next.dev.*` identifier. This keeps macOS
privacy grants attached across rebuilds. When more than one Apple Development
identity is available, select one explicitly:

```sh
SOYEHT_DEVELOPMENT_SIGNING_IDENTITY="Apple Development: Your Name (TEAMID)" \
  uv run python scripts/install_dev.py
```

The installer fails before copying a binary if it cannot establish and verify
the stable identifier, team identifier and designated requirement.

The installer creates only the `com.soyeht.next.dev.ptyd` and
`com.soyeht.next.dev.engine` services. Executables, database and logs live in
`~/Library/Application Support/Soyeht.next/dev/`. Service files live in
`~/Library/LaunchAgents/`. The engine listens on loopback and tailnet addresses
(IPv4 and IPv6), port 8291. If the tailnet becomes available later or changes
address, the engine detects the change and exits; launchd restarts it with the
current addresses. The supervisor stays alive, shells are adopted and clients
reconnect. The query uses only local system interfaces, without calling another program.

`pane create --shell --cwd PATH` opens the shell in that directory and creates
the corresponding workspace when needed. The current directory is the default.
To repeat creation with the same identity, use `--idempotency-key KEY`.
Responses are JSON; errors go to stderr with a nonzero exit code.

```sh
engine/target/debug/soyeht --profile dev pane close ID
engine/target/debug/soyeht --profile dev device pair-start
engine/target/debug/soyeht --profile dev device list
```

Pairing still requires consent in the Mac client. Displaying the code through
the CLI does not authorize a device by itself.

To replace the new profile's binaries, stop those two services first.
This terminates that profile's shells. The installer refuses to replace
a service that remains loaded.

```sh
launchctl bootout "gui/$(id -u)/com.soyeht.next.dev.engine"
launchctl bootout "gui/$(id -u)/com.soyeht.next.dev.ptyd"
cargo build --manifest-path engine/Cargo.toml --locked --workspace
uv run python scripts/install_dev.py
```

To update only the engine and preserve shells, stop only
`com.soyeht.next.dev.engine`, build and run
`uv run python scripts/install_dev.py --component engine`. The supervisor remains
active and the engine adopts the same processes.

Tests may use `--data-dir PATH` to connect to a temporary installation.
On a runner without a graphical domain, the installer accepts `--domain user`;
inspection and stop commands use that same domain.
None of these options accepts another profile or service name.

## Local quality ratchet

Once per machine, with network access, install the pinned copy/paste detector
with `npm ci --prefix scripts/quality --ignore-scripts` and the Rust unused-
dependency scanner with
`cargo install cargo-machete --version 0.9.2 --locked`. In a fresh worktree the
gate restores jscpd 4.3.0 from `scripts/quality/package-lock.json` with
`npm ci --offline`, using only the local npm cache. It finds `cargo-machete` on
`PATH` or in `$CARGO_HOME/bin` (default `~/.cargo/bin`). When a tool is absent or
has another version, the gate fails and prints the install command; it never
downloads a package. Periphery is optional and is reported as `SKIPPED` when
unavailable, not as a passing check.

The engine and shared Swift scans include tests. The Mac and iOS apps currently
have no Swift test directories, so their scans cover `Sources`; add any future
test targets to the scanner when they are created.

Enable the versioned fast pre-push hook with
`git config core.hooksPath .githooks`. It runs only
`uv run python scripts/check_quality.py`, not the full merge gate. The full
`bash scripts/check_all.sh` remains a manual pre-merge requirement.

The measurements must match `scripts/quality_baseline.json` exactly. More
clones or duplicated lines, a file growing past its cap, more loose literals in
a file, or more `unwrap`/`expect`/`panic!` calls in `engine/*/src` (inline
`#[cfg(test)]` items are skipped; test modules in separate files are counted)
fail as regressions. An improvement also fails
until the same PR lowers the baseline, so the gain cannot be spent later:
`uv run python scripts/check_quality.py --print-baseline > scripts/quality_baseline.json`.
Existing files above 800 lines have individual caps equal to their measured
lines; new files above 800 lines fail.

Raising any baseline entry above its value at the merge-base with `origin/main`
is a relaxation. Each one needs a written reason in
`scripts/quality_baseline_justifications.json`, keyed as the failure message
prints it, for the exact new value:
`{"oversized_files.engine/x.rs": {"value": 900, "reason": "..."}}`. Give
reviewers the output of `uv run python scripts/review_report.py BASE_COMMIT`; it
marks every relaxation with a red indicator and shows its reason.
