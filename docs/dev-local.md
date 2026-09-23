# Run the new Dev profile

GitHub Actions is disabled until the product is ready. Before every merge, run
`bash scripts/check_all.sh` with `FORBIDDEN_TERMS` supplied through the environment.
This local merge gate runs the workflow checks in sequence and stops at the first
failure. Record the results in the PR description. To also scan PR history and
the final title/body, pass `--base FULL_BASE_OID --event-file PATH_TO_EVENT_JSON`.
Keep the vocabulary outside the repository. The workflows remain available for
when hosted CI resumes.

Install the tools listed in `docs/toolchains.md`. Build from the repository root:

```sh
cargo build --manifest-path engine/Cargo.toml --locked --workspace
python3 scripts/install_dev.py
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
  python3 scripts/install_dev.py
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
python3 scripts/install_dev.py
```

To update only the engine and preserve shells, stop only
`com.soyeht.next.dev.engine`, build and run
`python3 scripts/install_dev.py --component engine`. The supervisor remains
active and the engine adopts the same processes.

Tests may use `--data-dir PATH` to connect to a temporary installation.
On a runner without a graphical domain, the installer accepts `--domain user`;
inspection and stop commands use that same domain.
None of these options accepts another profile or service name.

## Local quality ratchet

In each fresh worktree, before pushing or running `bash scripts/check_all.sh`,
install the pinned copy/paste detector with
`npm ci --prefix scripts/quality --ignore-scripts`. Install the Rust unused-
dependency scanner with `cargo install cargo-machete --locked` if it is missing.
The quality gate fails with these commands when a required tool is absent; it
never downloads a package from a Git hook. Periphery is optional and is reported
as `SKIPPED` when unavailable, not as a passing check.

The engine and shared Swift scans include tests. The Mac and iOS apps currently
have no Swift test directories, so their scans cover `Sources`; add any future
test targets to the scanner when they are created.

Enable the versioned fast pre-push hook with
`git config core.hooksPath .githooks`. It runs only
`python3 scripts/check_quality.py`, not the full merge gate. The full
`bash scripts/check_all.sh` remains a manual pre-merge requirement.

The checked-in `scripts/quality_baseline.json` must exactly match current
measurements. When a metric improves or an oversized file shrinks, lower its
baseline in the same PR. Existing files above 800 lines have individual caps
equal to their measured lines; new files above 800 lines fail. Any increase or
new exception is a baseline relaxation requiring written justification in the
PR. Run `python3 scripts/check_quality.py --print-baseline` to inspect current
measurements, then update the baseline deliberately. Give reviewers the output
of `python3 scripts/review_report.py BASE_COMMIT`; it marks every relaxation
with a red indicator.
