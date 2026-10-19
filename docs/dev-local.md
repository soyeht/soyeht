# Run the new Dev profile

GitHub Actions is disabled until the product is ready. Before every merge, run
`bash scripts/check_all.sh --tier fast` with `FORBIDDEN_TERMS` supplied through the
environment; after the merge `scripts/main_watch.py` runs the full tier on main through
`scripts/post_merge.py` and reverts a confirmed culprit (ADRs 0089 and 0092).
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
(IPv4 and IPv6), port 8291. Loopback is always on. The engine looks for tailnet
addresses every two seconds and starts or stops their listeners in place, without
a restart (ADR 0078). While no tailnet listener runs, the log has an
`engine.tailnet_unreachable` event with the reason, `engine.status` and the pairing
result carry `reachability`, and the Mac shows “Not reachable from your devices”.
The query uses only local system interfaces, without calling another program.

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

## The full gate after the merge, and the remote Linux host

`scripts/main_watch.py` is one pass of the continuous post-merge gate. It fetches
`origin/main`, runs `scripts/post_merge.py` once on the newest commit when main moved, and
writes `.cache/main-health/watch.json` (the last processed commit, the result, the culprit
and any revert). `--dry-run` is the default; `--apply` pushes and merges the revert of a
confirmed culprit. Run it from a loop or a timer with `FORBIDDEN_TERMS` in the environment
and `gh` authenticated; it holds a lock, so overlapping runs exit 75.

```sh
FORBIDDEN_TERMS=... uv run python scripts/main_watch.py --apply \
  --remote HOST --remote-root /path/to/soyeht
```

`--remote HOST --remote-root PATH` (also accepted by `post_merge.py`) runs the steps of the
areas engine, desktop and e2e on HOST and the other full-tier steps (the Swift packages, the
Mac and iOS apps, the editor measurement, the dictation tests, the timing tests, the
`always` checks) on the local machine, after the remote ones. The interface with the host is
this and nothing else:

- HOST is anything `ssh` accepts, with key authentication and no prompt (`ssh -o
  BatchMode=yes HOST ...`). `SOYEHT_REMOTE_SSH` replaces the `ssh -o BatchMode=yes` command
  (split on example).
- PATH is a clone of this repository on the host, with `origin` pointing at the same remote.
  It needs `scripts/remote_gate.sh`; keep the clone on a recent main (`git pull`), because
  the script that runs is the one in the clone, while the gate that runs is the commit's.
- Each run is `ssh HOST bash -lc 'cd PATH || exit 2; [ -f scripts/remote_gate.sh ] || exit 2;
  bash scripts/remote_gate.sh --sha FULL_SHA --steps ID,ID,... [--base FULL_SHA]'`. A login
  shell, so the host's toolchain is on `PATH`.
- `remote_gate.sh` fetches `origin main` (and the commit itself when that does not reach it),
  checks the commit out in the persistent worktree `PATH/.cache/remote-gate/tree` (untracked
  files go, ignored ones such as `target/` stay, so builds are incremental) and runs
  `bash scripts/check_all.sh --tier full --only STEPS` there, with the per-step times in
  `PATH/.cache/remote-gate/timings.json`. Its output is the gate's output and its exit code is
  the gate's, so `check_all: FAILED step ID` reaches the log. Exit 2 means the host could not
  run the gate (no checkout, no script, the commit cannot be fetched); ssh's own exit 255 is
  the same. Neither records anything or reverts anything.
- PATH may start with `~`, which is the host's home (quote it on the command line). A host
  with no credential for the hosting service is fed by a push mirror; its git and other
  settings come from `PATH/.cache/remote-gate/gate.env` (docs/platforms.md, "A Linux gate host").
- `remote_gate.sh --print-timings` prints that timings file; `post_merge.py` merges it into
  the record of the run.
- A timing-sensitive step (the `timing` area and `editor-performance:rust`, whose latency limits a
  loaded machine trips: 243 ms against 150 ms at a load of 60 to 140, 92 ms when quiet) records the
  load average of the host it ran on (`load` in the record; `local_load` is the Mac's, in every
  record, and `remote_gate.sh --print-load` is the host's). The one rerun of such a step that failed
  with a load over `LOAD_PER_CPU_LIMIT` (1.0 per processor, `SOYEHT_LOAD_PER_CPU_LIMIT`) goes to the
  remote host when it is a Mac-side step and `--remote` is given (and the host is quiet and can run
  it), else it waits for the load to drop. All the waiting of a run, the rerun and every probe of
  the culprit search together, shares `LOAD_WAIT_BUDGET_SECONDS` (30 minutes), and the ssh that
  reads the host's load times out after `LOAD_PROBE_TIMEOUT_SECONDS`. A pass is `flaky` as usual.
  The culprit search probes on the host the rerun used, waiting for it to be quiet before each probe.
  A host still busy when the budget is spent is inconclusive: a failed rerun is recorded `flaky` with
  that note (no culprit, the next pass measures again), and a probe that fails on a busy host (or one
  that turned busy) is skipped, so it gives candidates and reverts nothing. A probe that passes is
  good on any host, because load can only slow a step.
  The tradeoff: a regression that only the Mac shows in a `timing` step and that passes on Linux is
  recorded `flaky`, not red, so it does not advance `last_green.json`; nothing is reverted for it.
- The restricted vocabulary is not sent: the remote steps do not scan for it, and a
  placeholder satisfies the gate's check that it is set.
- One run at a time per clone (`post_merge.py` holds the lock). Set
  `SOYEHT_REMOTE_GATE_DIR` to move the state directory. The host needs the Rust toolchain,
  `uv`, `git` and whatever the e2e suites start; `scripts/check_engine_linux.sh` lists what
  the engine needs. On a host with Nix (NixOS) the script enters `scripts/linux-gate.nix`
  itself and needs none of that; `--slot NAME` adds a second persistent worktree
  (docs/platforms.md, "A Linux gate host").

### The editor measurement and a warm worktree

`post_merge.py` runs the gate in a worktree of its own, which has no
`.cache/editor-performance-vm.json`, the VM1 report. Pass the report with
`--editor-meter REPORT` (or `SOYEHT_EDITOR_METER`, also read by `main_watch.py`): it is copied into
the run's worktree before the gate. The report is valid for as long as the editor files it
measured are unchanged (`check_editor_performance.editor_tree_hash`).

Without a valid report for the commit, `editor-performance:check` fails with "missing ..." or
"editor paths changed; measure this editor version again on VM1". That is not a red main:

- the steps after the check still run (the gate stops at its first failure), and if one fails the
  run is red as usual;
- otherwise the run is recorded as `needs-measurement` with the `editor_tree_hash` to measure, in
  the health record and in `watch.json`, and the exit code is 4 (`main_watch.py` and
  `post_merge.py`). No culprit is searched and nothing is reverted;
- measure that editor version on VM1 (above), then run the watch again with `--editor-meter`: a
  commit recorded as `needs-measurement` runs again only when the report carries its hash;
- the Rust half of the editor check is its own engine step, `editor-performance:rust`, which runs
  before the check (so a missing report does not hide it) and, with `--remote`, on the Linux host;
- a measured latency at or over 8 ms on a matching hash stays red, and its culprit search treats a
  commit whose probe fails as stale or missing as skipped, not blamed.

`--worktree DIR` (or `SOYEHT_POST_MERGE_WORKTREE`) keeps one worktree between runs, so a run does not
build the Rust crates from cold. Each run checks the commit out there with
`git checkout --detach --force` and cleans it with `git clean -fdx`, keeping `engine/target`,
`apps/desktop/target`, `.cache/apple-packages` and `.cache/editor-performance-vm.json`; the culprit
search checks out its probes in the same directory. The Swift `.build` directories and Xcode's
derived data (`.cache/mac-derived`, `.cache/ios-derived`) are cleaned, not kept: SwiftPM's and
Xcode's incremental builds miss cross-module changes across checkouts and leave stale objects
("Undefined symbols" in `swift test`), while cargo's targets are safe to reuse.

DIR is the script's own. It is created on the first run, with a marker in its git directory, and
only a worktree with that marker is reused: the main checkout (or a path inside it), the checkout
the script runs from, a worktree you made yourself and any other directory are refused with exit 2,
and nothing in them is touched. A relative DIR is resolved against the working directory. A
worktree that cannot be prepared (a killed git left an `index.lock`, which the next run removes, or
a damaged one) is removed and created again. Never share it, or its target directories, with a
worktree you build in. Without the option each run uses a temporary worktree and removes it.

```sh
FORBIDDEN_TERMS=... uv run python scripts/main_watch.py --apply \
  --worktree ~/.cache/soyeht-post-merge --editor-meter /path/to/editor-performance-vm.json
```

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
`uv run python scripts/check_quality.py`, not the merge gate. The fast tier,
`bash scripts/check_all.sh --tier fast`, remains a manual pre-merge requirement.

The measurements must match `scripts/quality_baseline.json` exactly. More
clones or duplicated lines, a file growing past its cap, more loose literals in
a file fail as regressions. Engine and desktop-client production code may contain no
`unwrap`/`expect`/`panic!` calls at all: `engine_production_panics` is fixed
at 0, and no reason can raise it. Crate `tests/` and `benches/`, inline
`#[cfg(test)]`/`#[cfg(all(test, ...))]` items, and modules declared
`#[cfg(test)] mod x;` (with their submodules) are test code and are not counted.
An improvement also fails until the same PR lowers the baseline, so the gain
cannot be spent later:
`uv run python scripts/check_quality.py --print-baseline > scripts/quality_baseline.json`.
Existing files above 800 lines have individual caps equal to their measured
lines; new files above 800 lines fail.

Raising any other baseline entry above its value at the merge-base with `origin/main`
is a relaxation. Each one needs a written reason in
`scripts/quality_baseline_justifications.json`, keyed as the failure message
prints it, for the exact new value:
`{"oversized_files.engine/x.rs": {"value": 900, "reason": "..."}}`. Give
reviewers the output of `uv run python scripts/review_report.py BASE_COMMIT`; it
marks every relaxation with a red indicator and shows its reason.
