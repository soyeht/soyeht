# Run the new Dev profile

GitHub Actions is disabled until the product is ready. Before every merge, run
`bash scripts/check_all.sh` with `FORBIDDEN_TERMS` supplied through the environment.
This local merge gate runs the workflow checks in sequence and stops at the first
failure. Record the results in the PR description. To also scan PR history and
the final title/body, pass `--base FULL_BASE_OID --event-file PATH_TO_EVENT_JSON`.
Keep the vocabulary outside the repository. The workflows remain available for
when hosted CI resumes.

The editor performance proof is collected separately on VM1, from the same
checkout as the PR. Set `SOYEHT_EDITOR_METER_VM=1` and a locally configured
`SOYEHT_DEVELOPMENT_TEAM` in the VM (never commit either value), then run
`python3 scripts/check_editor_performance.py --measure`. The command builds
and UI-tests a signed Release app three times; it does not run on the host.
Copy `.cache/editor-performance-vm.json` to the review checkout's `.cache`
before `check_all.sh`. Configure the VM's SSH address outside the repository
(for example, `SOYEHT_VM_SSH`), never in a script or report. The local gate
rejects a missing/Debug/slow report or one whose editor Mac, SoyehtClient,
protocol, or meter files changed. An engine-only commit can reuse a still-valid
report. Key and ordinary-scroll work each require median p95 < 8 ms across the
three runs; every run asserts real-screen text ink, aligned gutter, 240 typed
characters, movement, and at least 20 page changes. Page-load timing remains
diagnostic under #404; the old CPU bitmap stopwatch is gone.

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
