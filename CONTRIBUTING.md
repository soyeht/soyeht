# Contributing to Soyeht

Soyeht runs coding agents as a team: each agent harness gets its own terminal or agent
pane, agents find and message each other over MCP, and sessions survive restarts. It is
meant to be a platform you download and then change, including Soyeht's own code, and
share what you built. The engine and the clients are built from this repository on macOS,
Linux and Windows.

Open an issue before a large change. Every feature starts in `docs/contracts/`; if it is
not in a contract, it is not in scope yet. Bugs and small fixes can go straight to a pull
request.

## Where things live

`AGENTS.md` is the one place for the layer map and the rules (single source of truth in
`contracts/`, no duplication, no god files, the Core lock, design tokens, repository
hygiene). Read it before you write code; this file does not repeat it. In short:

| Path | What it is |
| --- | --- |
| `contracts/` | The protocol, profiles and design tokens, and the vectors both clients pass. Generated code comes from here. |
| `engine/` | The Rust engine (a cargo workspace): protocol, platform seams, process supervisor, store, the domain crates, the engine, the CLI and the MCP server. |
| `apps/shared`, `apps/dictation` | Swift packages shared by the Mac and iPhone apps. |
| `apps/mac`, `apps/ios` | The native apps, thin views over `apps/shared`. |
| `apps/desktop` | The Rust and GPUI client for Linux and Windows (also runs on macOS). Its own cargo workspace. |
| `docs/` | Contracts (`docs/contracts/`), decisions (`docs/adr/`), platform and dev notes. |

Guides for the two most common extensions:

- [Add a pane type](docs/add-a-pane-type.md)
- [Add a harness](docs/add-a-harness.md)

## Prerequisites

Everything needs `git` and these tools:

- Rust, through [rustup](https://rustup.rs). `rust-toolchain.toml` pins the version and
  installs it on first use inside the repository; do not change your global default.
- [uv](https://docs.astral.sh/uv/) for every Python script (`uv run python ...`, never a
  bare `python3`). The scripts need Python 3.11 or later and only the standard library.
- Node.js with npm: the TypeScript protocol package (`contracts/typescript`, whose tests run
  `.ts` files directly with `node --test`) and the duplication scanner of the quality check.
  Use the version CI pins, `node-version` in `.github/workflows/check-all.yml`; the repository
  declares no other minimum.
- `bash` and `perl`: `scripts/check_all.sh` is a bash script.

Per system:

| System | Also needed | Details |
| --- | --- | --- |
| macOS 15 or later | Xcode 26.6 (Swift 6.3.3) for `apps/shared`, `apps/dictation`, `apps/mac` and `apps/ios`. The desktop client on macOS also needs Xcode with the Metal toolchain. | `docs/toolchains.md`, `docs/apps-mac.md` |
| Linux (Ubuntu, Fedora) | A C toolchain (`build-essential` on Ubuntu, `gcc` and `make` on Fedora) and `cmake`, `clang`. For `apps/desktop`: the Wayland, X11, xkbcommon, fontconfig, freetype, OpenSSL and ALSA development packages, and a Vulkan driver to run it. | "Running the Linux check" and "Graphics on Linux" in `docs/platforms.md` list the package names for both distributions |
| Windows 11 | Visual Studio C++ build tools (MSVC and a Windows SDK), Git for Windows, and on ARM64 LLVM. An elevated shell or Developer Mode, because tests check that symbolic links are never followed. | "Running the Windows check" in `docs/platforms.md` |

On Windows, `scripts/check_all.sh` needs a bash (Git for Windows or WSL). The maintainers'
gate does not run on Windows; the engine and desktop checks there are
`scripts/check_engine_windows.ps1`. Tell us in an issue if a step does not work for you.

The macOS apps are the only part that needs Xcode, and the only part that needs an Apple
Development identity (to run them signed). Nothing else does.

Tools for the quality check, installed once (it never downloads at run time):

```sh
npm ci --prefix scripts/quality --ignore-scripts
cargo install cargo-machete --version 0.9.2 --locked
```

`scripts/quality_core.py` holds the pinned versions.

## Build

From the repository root, on any system:

```sh
cargo build --manifest-path engine/Cargo.toml --locked --workspace
```

Build the whole workspace, not only `soyeht-engine`: the engine refuses to open an agent
whose `soyeht-mcp` program is not installed next to it. The desktop client is built on its
own (`apps/desktop/README.md`):

```sh
cargo build --manifest-path apps/desktop/Cargo.toml --locked --release -p soyeht-desktop
```

The first desktop build fetches the GPUI git dependencies and takes several minutes. The
Mac and iPhone apps build with `bash scripts/check_mac.sh` and `bash scripts/check_ios.sh`
(`docs/apps-mac.md`, `docs/apps-iphone.md`).

## Run the engine and a client in a dev profile

The engine is two programs: `soyeht-ptyd` (it holds the pseudoterminals) and
`soyeht-engine`. Start them from the build directory with a throwaway data directory, so
nothing of your own profile is touched. Keep the path short: a Unix socket path has a limit
of about a hundred bytes.

macOS and Linux:

```sh
DATA=/tmp/soyeht-dev
mkdir -p "$DATA"
engine/target/debug/soyeht-ptyd --data-dir "$DATA" &
engine/target/debug/soyeht-engine --data-dir "$DATA" --listen 127.0.0.1:0 &
```

Windows (PowerShell):

```powershell
$Data = "$env:TEMP\soyeht-dev"
New-Item -ItemType Directory -Force $Data | Out-Null
Start-Process engine\target\debug\soyeht-ptyd.exe "--data-dir", $Data
Start-Process engine\target\debug\soyeht-engine.exe "--data-dir", $Data, "--listen", "127.0.0.1:0"
```

Then talk to it with the CLI, or open the desktop client on it:

```sh
engine/target/debug/soyeht --profile dev --data-dir "$DATA" status
engine/target/debug/soyeht --profile dev --data-dir "$DATA" pane create --shell
engine/target/debug/soyeht --profile dev --data-dir "$DATA" pane list

SOYEHT_ENGINE_SOCKET="$DATA/engine.sock" cargo run --manifest-path apps/desktop/Cargo.toml --release -p soyeht-desktop
```

On Windows, the desktop client finds the engine's endpoint file at the same path:

```powershell
$env:SOYEHT_ENGINE_SOCKET = "$Data\engine.sock"
cargo run --manifest-path apps\desktop\Cargo.toml --release -p soyeht-desktop
```

(The Windows commands in this section follow the notes in `docs/platforms.md` and have not
been run by the maintainers; their Windows check is `scripts/check_engine_windows.ps1`.)

`soyeht --help` lists every command. `--listen 127.0.0.1:0` keeps the engine off the fixed
port 8291 and off your tailnet. Without `--data-dir` all three programs use the Dev
profile directory under your home (`~/Library/Application Support/Soyeht.next/dev`, on
every system), which is where the Mac app and the desktop client look by default.

On macOS the Mac app uses only that default profile. Install the engine into it as launchd
services with `uv run python scripts/install_dev.py`; it signs the binaries with your Apple
Development identity (`docs/dev-local.md`, `docs/apps-mac.md`). Stop what you started when
you are done: end the two processes (on Windows, `Stop-Process -Name soyeht-engine, soyeht-ptyd`).

## Tests

| What | Command |
| --- | --- |
| Engine, everything | `cd engine && cargo test --locked --workspace` |
| One engine crate | `cd engine && cargo test --locked -p soyeht-store` |
| Engine with the contract checks the gate uses | add `--features soyeht-engine/contract-checks` |
| Desktop client | `cd apps/desktop && cargo test --locked --workspace` |
| Generated code and generator | `uv run python contracts/generate.py --check` and `uv run python -m unittest discover -s contracts -p 'test_*.py'` |
| TypeScript package | `cd contracts/typescript && npm ci --ignore-scripts && npm test` |
| Swift shared client (macOS) | `(cd engine && cargo build --locked --workspace) && swift test --package-path apps/shared` |
| Swift dictation (macOS) | `swift test --package-path apps/dictation` |
| Gate scripts | `uv run python -m unittest discover -s scripts -p 'test_check_quality.py'` (one file per run) |
| End to end (needs the built engine) | `uv run python scripts/e2e.py --report .cache/e2e.json`, and `e2e_agents.py`, `e2e_tasks.py`, `e2e_editor.py`, `e2e_web.py`, `e2e_writing.py` |

The end-to-end scripts start their own disposable engine; they never touch an installed
profile. The Linux and Windows engine and desktop checks are
`scripts/check_engine_linux.sh` and `scripts/check_engine_windows.ps1`.

Tests that need a POSIX shell are marked `cfg(unix)` (`docs/platforms.md`, "Tests"). Put a
test next to the rule it proves. A new rule needs a new test, and the gate flags a drop in the
number of tests.

## Before you open a pull request

1. Branch from an up-to-date `main`, and fetch it, because the ratchets compare with the
   merge-base of `origin/main` and `HEAD` and fail if it cannot be resolved:
   `git fetch origin main`. (On a fork, keep your fork's `main` in sync with upstream.)
2. If you changed `contracts/`, regenerate and commit the outputs with their source:
   `uv run python contracts/generate.py`. Never edit a file whose first line starts with
   `// Generated from contracts/` or `# Generated from contracts/`.
3. Run the fast check, which the pre-push hook also runs
   (`git config core.hooksPath .githooks`):

   ```sh
   uv run python scripts/check_quality.py
   ```

4. Run the precheck. It needs no build or vocabulary and reports every failure at once: the
   cheap repository checks, AI attribution in the commit messages, Swift format on the files you
   changed, and the contracts checks. Before opening the pull request, save its title and body
   to a file and pass it too:

   ```sh
   uv run python scripts/agent_precheck.py --pr-text pr.md
   ```

   Then run the merge gate's fast tier. It runs format, lint, build and unit tests for the areas
   your change touches, plus every cheap repository check:

   ```sh
   SOYEHT_SKIP_FORBIDDEN_TERMS=1 bash scripts/check_all.sh --tier fast
   ```

   `uv run python scripts/gate_plan.py --tier fast` prints what that run would select for your
   change, and whether a review is required, without running anything. The areas that need
   Xcode (the Swift packages, the Mac and iOS builds) run only where you touched them and
   only on macOS; if you changed `apps/mac` or `apps/ios`, also run `bash scripts/check_mac.sh`
   or `bash scripts/check_ios.sh`.
5. If a number in `scripts/quality_baseline.json` improved (fewer clones, smaller files,
   fewer swallowed errors), lower it in the same pull request:
   `uv run python scripts/check_quality.py --print-baseline > scripts/quality_baseline.json`.
   Raising a baseline needs a written reason for the exact value in
   `scripts/quality_baseline_justifications.json`.
6. Commit messages, the pull request title and body are in English and carry no AI
   attribution: no co-author trailers, no "generated by" lines.

### The vocabulary scan

The merge gate also scans the repository, the commits and the pull request text for a list of
restricted words that the maintainers keep outside the repository (`FORBIDDEN_TERMS`).
You do not have, and do not need, that list. `SOYEHT_SKIP_FORBIDDEN_TERMS=1` makes
`scripts/forbidden_terms.py` skip the vocabulary and print a loud banner saying so; it still
rejects AI-attribution phrases in commit messages. A maintainer runs the full scan before
merging. If `FORBIDDEN_TERMS` is set, the flag is ignored and the real scan runs.

## Review and merge

`gate_plan.py` prints `review: required` when a change touches the engine core, the
protocol, security-sensitive code, dependencies, a ratchet exception or the gate scripts
(the list is in `AGENTS.md`, "Commands"). A required review is done by a second person
before the merge. Otherwise a pull request merges on a green fast tier. After the merge the
maintainers' watcher runs the full tier on `main` and reverts a commit it proves to be red
(`docs/dev-local.md`), so expect to re-land a fix if yours is reverted.

## Reporting security problems

Do not open a public issue. See [SECURITY.md](SECURITY.md).

## License

By contributing you agree that your contribution is licensed under Apache-2.0 (`LICENSE`).
