# Rules for agents working in this repository

These rules are enforced by the merge gate where a check exists. Where the gate
cannot see a rule yet, the rule still applies. If code and this file disagree,
fix one of them in the same PR.

## Architecture layers

Each layer depends only on the layers listed above it.

1. `contracts/`: `protocol.json` (wire methods, events, limits, the clients' shared policy,
   error codes),
   `profiles.json` (installed profile values) and `design-tokens.json` (palettes,
   radii, spacing, type scale, shadows, layout metrics, fonts). This is the only
   place where wire values and design tokens are defined.
2. `engine/soyeht-protocol`: generated wire models, limits and framing, plus the
   transport-agnostic rules every client shares: the greeting and `hello`
   validation (`handshake`) and snapshot page assembly (`snapshot`). It has no
   Soyeht dependencies and no IO or async runtime.
   `engine/soyeht-platform` sits at the same level: the operating-system seams
   (the local endpoint, owner-only files, process launch and termination, the user's
   identity, the structured log line). It has no Soyeht dependencies, and these system
   calls live there (ADR 0086). A crate above it may keep a `cfg` module for a concern
   specific to its own job (`soyeht-ptyd`'s child and pty, the engine's log rotation, the
   desktop's `os_notify`), under the rule below: the same public shape on every system.
   `engine/soyeht-process`, the bounded runner for every external command the engine
   starts, depends on nothing but `soyeht-platform` (ADR 0079).
3. Engine building blocks, each depending only on `soyeht-protocol` and
   `soyeht-platform`, and on `soyeht-process` where they run a command:
   - `soyeht-ptyd`: the process supervisor that holds pseudoterminals.
   - `soyeht-acp`: the ACP v1 adaptation. Process ownership and durable effects
     belong to the engine.
   - `soyeht-store`: the only crate with SQL or `rusqlite`
     (`hardening_gate.py check-no-sql-outside-store`).
   - Pure, bounded domain crates: `soyeht-editor`, `soyeht-writing`,
     `soyeht-web`, `soyeht-pane-state`, `soyeht-usage`, `soyeht-lsp`, and
     `soyeht-vcs` (whose only Soyeht dependencies are `soyeht-platform`, for
     canonical paths, and `soyeht-process`).
   - `soyeht-skills`: the skill folders (`SKILL.md`) shared by every harness.
4. `engine/soyeht-engine`: assembles the crates above behind one local socket
   and WebSocket API. The `soyeht-cli` binary uses protocol, platform, ptyd and
   store; the `soyeht-mcp` binary uses protocol, platform and skills. Neither
   depends on the engine crate.
5. `apps/shared` (`SoyehtClient`): the shared Swift client. It holds the engine
   client, replicas, presentation policies, layout math and the theme. Put logic
   here so `swift test --package-path apps/shared` covers it.
   `apps/dictation` (`SoyehtDictation`) is a second package on top of it: the
   dictation engines that need third-party dependencies, and the catalogue of
   engines (ADR 0083).
6. `apps/mac/Sources` and `apps/ios/Sources`: thin views over `SoyehtClient`.
   They render and forward input. They do not own state rules or wire handling. The Mac
   creates and injects every observable environment object once, in `SoyehtMacApp.swift`
   (`check_mac_environment.py`).
7. `apps/desktop`: the Rust client for Linux and Windows (GPUI, ADR 0084). It is
   its own cargo workspace and depends on `engine/soyeht-protocol` and, for the local endpoint
   (the Unix socket, and on Windows the loopback endpoint file), `engine/soyeht-platform`. Its
   `soyeht-client` crate holds the connection and state, `soyeht-desktop` the
   views, and its `theme` module holds the palettes and tokens generated from
   `contracts/design-tokens.json`, the same source the Swift side is generated from.
   Three seams belong to the window, each the same on every system: the log also goes to a
   rotating file (`soyeht_platform::log::to_file`, ADR 0101); a window that finds no engine
   starts `soyeht-ptyd` and `soyeht-engine` detached (`soyeht-client`'s `autostart` through
   `soyeht_platform::process::detached`, ADR 0102); and `os_notify` raises the system
   notification when an agent needs the person (ADR 0103).

`scripts/check_layers.py` reads every `Cargo.toml` of `engine/` and `apps/desktop/` and
fails when a crate depends on a Soyeht crate outside its layer, or when a new crate is not
in its `LAYERS` table. Place a new crate there and in the list above.

Every feature starts in `docs/contracts/`. Decisions live in `docs/adr/`; add an
ADR when you change a boundary.

## Single source of truth

- `contracts/protocol.json`, `contracts/profiles.json` and `contracts/design-tokens.json`
  are the sources.
  `uv run python contracts/generate.py` writes the Rust, Swift, TypeScript and
  Python outputs. A file whose first line starts with `// Generated from
  contracts/` (or `# Generated from contracts/`) is an output: never edit it by
  hand. Commit the outputs with their source; the gate runs `--check`.
- The `desktop` section of `design-tokens.json` is Rust-only and last in the file: window
  frame, popup gaps, glyph sizes and the sizes of views the Mac does not have. A size both
  clients draw (a sheet, a popover, a field, a column) is a token of a shared section
  (`layout`, `composer`, ...) that the Mac reads as `DesignTokens`; the same number as a Mac
  literal and a `desktop` token is the copy this rule stops. `contracts/test_design_tokens.py`
  proves a token that replaces a Mac literal against the pre-generation Mac source
  (`INLINE_LITERALS`), and a token with no such source is listed in `ADDED_AFTER_GENERATION`.
- Once a generated form exists, use it. Do not hand-type a method or event name,
  a limit, a frame kind or an error code. Examples: `RequestMethod`, `EventName`,
  `FrameKind` and `ProtocolLimits` in Swift; `method::*`, `event::*`, `limits::*` and
  `error_code()` in Rust. `scripts/check_wire_names.py` fails on a string literal equal to a
  method name, event name or error code in production code (`ErrorKind` is the generated
  form of the codes; the supervisor's own kinds live in `soyeht_ptyd::wire_kind`).
  `scripts/check_doc_names.py` fails when a document in `docs/` or `contracts/` (dated
  `docs/reports/` excepted) names a method, event, setting or field that does not exist.
- The engine builds an event from the generated type of its schema: the `...Draft` types
  (without the `eventId`, `at` and `actor` the store assigns) and `model::pending`, never a new
  `json!` literal with a `"paneId"` key. `scripts/check_event_literals.py` holds the count of
  those literals down. The `contract-checks` feature (ADR 0090) checks every event and result
  the engine sends against the contract; the merge gate builds and tests with it, so a field
  the contract does not have fails the request or, for a background publication, the engine
  integration rig and the end-to-end run that read the engine log.
- Rules every client must share that are not wire values (the Markdown subset of agent
  replies, ADR 0087) are vector files in `contracts/vectors/`, and each client's tests pass
  the same file. Change the vectors first.
- The rules the Mac, iOS and desktop clients share are `x-clientPolicy` in `protocol.json`:
  request, greeting and reconnect timings, the terminal input chunk, the `caps` (waiting messages
  read, links found in one text, pages of a model catalogue, tool calls the ticker lists), the
  usage and context band thresholds and ring opacities, the weekly-window rule, provider names and which
  configuration selector is the model, effort or permission one. Use `ClientPolicy` and
  `ConfigOption.isModel`/`isEffort`/`isPermissionMode` in Swift and
  `soyeht_protocol::client_policy` in Rust. Behaviour the two languages implement separately
  (the usage presentation, the pane layout and each kind of pane's floor, of which the desktop
  holds only the agent's) is held together by the cases of
  `contracts/vectors/` that both pass. Theme names, their order and display names are the
  `palettes` of `design-tokens.json`; `TerminalColorTheme` and `ThemeName` are generated.
- One rule, one place. Before you add a helper, a constant or a policy, search
  for an existing one and reuse or extend it.
- Canonical paths come from `soyeht_platform::fs::canonicalize` only. The standard
  `canonicalize` gives Windows' `\\?\C:\...` form, which `cmd.exe` and `git` read differently;
  `engine/soyeht-platform/tests/canonical_paths.rs` fails on any other use.

## No duplication and no god files

`scripts/check_quality.py` must match `scripts/quality_baseline.json` exactly:

- jscpd clones and duplicated lines per area may not grow. The areas are the engine, the
  desktop (and its views at a smaller window), each Swift package, and the Python of
  `scripts/` and `contracts/`.
- A new file may not exceed 800 lines. A file already above 800 lines may not
  grow past its recorded cap. Split it instead.
- Loose Swift padding, corner-radius, system font-size and font-name literals, and the
  numbers `4096` and `512` anywhere in a Rust line of the engine or `apps/desktop` (code, test
  or comment; `limits.rs` and generated files are exempt), may not grow per file. Use a
  token, a policy or the generated limits.
- Engine and `apps/desktop` production code has no `unwrap`/`expect`/`panic!`
  calls. The limit is 0, and no baseline edit or written reason can raise it.
  Test code is exempt:
  crate `tests/` and `benches/`, inline `#[cfg(test)]` and
  `#[cfg(all(test, ...))]` items, and modules declared `#[cfg(test)] mod x;`,
  including their submodules. Clippy denies these calls through the workspace
  lints too.
- Swallowed errors (`swallowed_errors`): Swift `try?` and Rust `.ok()` / `let _ =` /
  `Err(_) =>` in production code may not grow. Log, propagate or surface the error
  instead, or keep it with a one-line comment saying why ignoring it is correct.
  `uv run python scripts/check_swallowed_errors.py` lists every counted site.
- Logging: every log line is written by `soyeht_platform::log` (`event`, `failure`,
  `failure_with`, or a `Line` built now and emitted later). A failure logs its kind, never
  its message text, which can carry paths. A failure that can repeat keeps an engine
  `Streak` (`streak.rs`): it carries the retry backoff and says when to log, which is the
  first failure and each change of kind; the caller logs the recovery when it wants to.
  A repeating condition that is not a failure follows the same idea: a slow Core lock hold
  logs the first, a hold twice as long, and a summary each minute with the count and the
  worst (`slow_holds.rs`).
  `scripts/check_logging.py` rejects `eprintln!` in `engine` and `apps/desktop` `src`
  outside `soyeht-platform/src/log.rs` and a binary's `main.rs` (ADR 0088).
- Dead code: `scripts/check_dead_pub.py` fails on a `pub fn` of an engine or `apps/desktop` crate
  that no production code references (`pub` hides it from the compiler's unused-code lint). Delete
  it and the tests that only exercise it. An entry of `scripts/dead_pub_allowlist.json`
  needs a reason, and fails once it is no longer needed.
  `scripts/check_dead_public_swift.py` does the same for a `public` declaration of `apps/shared`
  that no Swift code in `apps`, the tests or the generated `contracts/swift` refers to, with
  `scripts/dead_public_swift_allowlist.json` for the names a framework calls.
- When a number improves, lower the baseline in the same PR:
  `uv run python scripts/check_quality.py --print-baseline > scripts/quality_baseline.json`.
  The check fails until you do.
- Raising any other baseline entry needs a written reason for the exact new value in
  `scripts/quality_baseline_justifications.json`. `review_report.py` shows each
  reason to the reviewer.

`scripts/hardening_gate.py` also fails when the number of functions over 120 or
over 200 lines grows, when an existing long function grows, or when tests,
acceptances or gate steps drop. A drop needs a `Verification change:` line in the
PR body.

## The Core lock

The engine holds all state behind one Core lock, which the terminal output path
also takes. Never hold it across blocking IO: an external command, a network
request or a directory walk (ADR 0079). Copy the inputs under the lock, run the
work through `Outcome::Offload` (a request) or `Core::spawn_background` (engine
work), and apply the result under the lock. Run every external command through
`soyeht_process::run`, which has a deadline and refuses to start under the lock.
`tests/blocking_io_rule.rs` and `tests/core_lock.rs` enforce this.

## Test hooks

Production engine code holds no behaviour that exists only for a test: no `cfg(debug_assertions)`,
no marker file named `test-...` and no `SOYEHT_TEST_...` variable (ADR 0091), because the app a
developer installs is a debug build. A test that needs the running engine to misbehave adds a
method with a default to `soyeht_engine::hooks::Hooks`, which `Core` receives through
`Core::open_with`, and implements it in `test_hooks::FileHooks`. That module, `usage::FixtureHost`
and the `soyeht-engine-test` program exist only with the `test-hooks` feature, which no default
build enables. Unit tests pass their own hook to `Core::open_with`. `scripts/check_no_test_backdoors.py`
enforces it, with an allowlist whose entries each need a reason.

## How to add an STT engine

Dictation engines are a catalogue (ADR 0083). A new engine is one file and one entry:

1. Add one file to `apps/dictation/Sources/SoyehtDictation/` with a `SpeechRecognizing`
   implementation and its `DictationEngine` descriptor (id, names, platforms, languages,
   `usesVocabulary`, `model` when it needs an install, `availability`, `make`). Reuse
   `MicrophoneCapture`, `AudioResampler`, `SystemCallback` and the installer of
   `DictationModelStore`; write no permission request, install state or vocabulary store.
   An engine with a model is opt-in: nothing downloads before the person presses Install.
2. List the descriptor in `DictationCatalogue.registry`.
3. Add tests with a fake engine.

Do not edit the composer, Preferences or `DictationController` to add an engine: they are
generated from the registry. System callbacks and audio taps are `@Sendable` and never
inherit the main actor.

## Design

- Colors come only from the palette: `SoyehtThemeColors`/`MacTheme`, backed by
  `ThemePalette.swift` and the palettes generated from `contracts/design-tokens.json`
  (ADR 0065). Radii, spacing, type sizes, shadows and layout metrics are tokens of the
  same file (`DesignTokens` in Swift, `theme::{radius, space, ...}` in Rust); to change
  one, edit the JSON and regenerate. Do not add literal or system colors, and do not
  add new colors. `check_mac_view_colors.py` enforces this.
  On the desktop, `check_desktop_tokens.py` rejects a size derived from a token by hand
  (`type_scale::BODY - 1.0`) or a numeric neumorphic radius: name the size in the JSON.
- Pane interiors are flat. `paneShell()` is the only raised (neumorphic) surface.
  Inside a pane, use no shadows, no raised or inset modifiers, and palette roles
  only. `check_pane_interior.py` enforces this.
- Use only the approved fonts (`check_mac_text_fonts.py`).
- A Mac `Text` literal that interpolates an identifier (a port, a pid, a line number) does so
  explicitly (`check_mac_identifier_text.py`).

## Commands

- Python: always `uv run python ...`, never a bare `python3`.
- Before every push an agent runs `uv run python scripts/agent_precheck.py` (`--quick` leaves out
  the minutes-long quality scan). It needs no build and no vocabulary and reports every failure
  at once: the cheap `always` checks of `check_all.sh --list`, the commit messages (and, with
  `--pr-text FILE`, the PR title and body you are about to give `gh pr create`) for AI
  attribution, `swift format lint --strict` (100 columns, `.swift-format`) on the changed Swift
  files, `contracts/generate.py --check` with the contracts tests, then `check_quality.py`.
  Do not skip the forbidden-terms step: with no vocabulary it still scans messages.
- Fast check, also run by the pre-push hook
  (`git config core.hooksPath .githooks`, which also scans the commit messages):
  `uv run python scripts/check_quality.py`.
  Its duplication scan has a deadline of 300 s that grows with the host load (up to 1800 s),
  so a busy machine does not fail it; a real hang still does. `SOYEHT_SCAN_TIMEOUT` (seconds)
  sets the deadline.
- Merge gate, before every merge:
  `FORBIDDEN_TERMS=... bash scripts/check_all.sh --tier fast` (ADRs 0089 and 0092). Get the
  vocabulary from outside the repository. `scripts/check_all.sh` is the only list of
  gate steps, and each step has an area and a tier. The fast tier is cheap (a few minutes on a
  warm cache): every `always` check plus, for the areas that `scripts/gate_plan.py` selects
  from the changed paths (the plan is the top of the log), format, lint, a build and the
  tests: `cargo test --lib --bins` of the engine crates changed and the crates that depend on
  them, the whole `cargo test` of the desktop (its `tests/` directories included),
  `swift format lint` and `swift build` of `apps/shared`, and TypeScript. The e2e suites
  (`e2e:*`, with the `engine:build-product` and `engine:build-test-engine` builds and the TCC
  guard around them) run when the engine selection is the whole workspace or its reverse
  closure holds `soyeht-engine`, `soyeht-cli`, `soyeht-ptyd` or `soyeht-mcp`, which every
  engine crate does. The unit tests of the gate scripts (`post_merge*`, `main_watch*`,
  `check_quality`, `gate_list`, `gate_timings`) are the `gate_scripts` area: a change under
  `scripts/`, `contracts/*.py` or `.githooks/` selects it. The fast tier leaves out the engine
  integration tests (`tests/` directories), the Swift and dictation tests, the Mac and iOS app
  builds, the editor measurement on the VM and the ignored timing tests: they are `full`
  steps. A change to the gate scripts (`check_all.sh`,
  `gate_*`, `post_merge*`, `main_watch*`, `remote_gate*`, `e2e*`, the app build scripts) runs
  the full tier instead, and the log says why; a change to their unit tests alone
  (`scripts/test_*.py`) does not. A file that another directory reads (an `include_str!`, a
  font) goes in `CROSS_READS` of `gate_plan.py`; a test fails when a read is missing. A CI
  workflow calls the script and never lists steps of its own (`test_gate_list.py`).
  `check_all.sh --list --tier fast` shows the steps without running them. Moving a step from
  `both` to `full`, or out of the `always` area, lowers the per-area count the verification
  ratchet watches and needs a `Verification change:` line in the PR body.
- Two guards keep a green step honest: a step that names one Rust test passes only when exactly
  that test ran (`require_one_rust_test.py`), and the e2e steps fail when the run made macOS ask
  for a privacy permission on behalf of the engine's executables (`tcc_guard.py`).
- A contributor without the vocabulary sets `SOYEHT_SKIP_FORBIDDEN_TERMS=1`: `forbidden_terms.py` then
  skips the vocabulary scan with a loud banner, still rejects AI attribution in commit messages, and
  `check_all.sh` repeats the reminder at the end. Supplied `FORBIDDEN_TERMS` always win, and
  `post_merge.py` still refuses to run without them. A maintainer runs the real scan before the merge
  (`CONTRIBUTING.md`).
- Full gate, after the merge: `scripts/main_watch.py` (one pass; the coordinator runs it
  continuously) fetches `origin/main` and, when it moved, calls `scripts/post_merge.py` once
  on the newest commit. That runs `check_all.sh --tier full` in a clean worktree and writes
  `.cache/main-health/<sha>.json` (result, duration, per-step times) and `latest.json`. It reads
  the `Verification change:` line from the squash commit message, as the pre-merge gate reads it
  from the PR body, so the merge must keep that line in the commit body. When it
  is red it reruns the failing step once (a pass is recorded as flaky and blames no commit),
  then finds the culprit commit by running only the failing step on the commits since the last
  green main (`last_green.json`: a `pass`, or a `needs-measurement` run, where every step passed
  and only the editor measurement on the VM is missing). `main_watch.py --apply` then reverts a
  single confirmed culprit: a branch `main-watch/revert-<sha>`, a pull request "Revert <subject>" naming the failing step and the
  log, merged with `gh pr merge --squash`. It does not revert an ambiguous culprit (candidates),
  a flaky run, a revert, or a revert that does not apply, and it never forces a push or touches
  another branch; `--dry-run` is the default. It writes `.cache/main-health/watch.json`. With
  `--remote HOST --remote-root PATH` the engine, desktop and e2e steps run on a Linux host
  through `scripts/remote_gate.sh` (`docs/dev-local.md`). The plan prints a warning while
  `latest.json` says main is red. The editor measurement on the VM is collected separately,
  when the editor hash changes. Run `--tier full` yourself before a merge when you want
  certainty.
- Red main is fixed by the automatic revert of a confirmed culprit. Its author re-lands a
  fixed change (a new pull request through the fast gate); when the revert could not be made,
  they fix forward or revert by hand within the hour. No other change merges into the
  affected area while main is red.
- Review: `gate_plan.py` prints `review: required` when a change touches the engine core
  (`soyeht-engine` `core*`, `dispatch*`, `server*`, `agents/`, and `soyeht-store`), the
  protocol and `contracts/profiles.json`, an editor, security-sensitive code (all of
  `soyeht-platform/src`, `soyeht-ptyd`, `auth`, `pairing`, `push`, `network`,
  `ui_attestation`, web permissions), dependencies (`Cargo.toml`, `Cargo.lock`,
  `Package.swift`), a ratchet exception (`quality_baseline_justifications.json`,
  `dead_pub_allowlist.json`) or the gate scripts and their tests. A required review is done by a second
  agent before the merge. An `optional` change merges on a green fast tier with no separate
  reviewer.
- Build cache: when `sccache` is installed (`cargo install sccache --locked`), `check_all.sh`
  uses it with `CARGO_INCREMENTAL=0`. Give every worktree its own target directory, never a
  shared one.
- Step times of every run go to `.cache/gate-timings.json`
  (`uv run python scripts/gate_timings.py top`).
- The ratchets compare with the merge-base of `origin/main` and `HEAD`. They fail
  if it cannot be resolved, so run `git fetch origin main` first. Pass
  `--base FULL_OID` to override it.
- Rust: `export PATH="$HOME/.cargo/bin:$PATH"; (cd engine && cargo build --workspace)`.
  Add `--features soyeht-engine/contract-checks` to build or test with the contract checks on, as
  the merge gate does.
- Linux and Windows engine checks (not gate steps): `scripts/check_engine_linux.sh` and
  `scripts/check_engine_windows.ps1`, or `scripts/vmpal_check_engine.sh` from a Mac
  (`docs/platforms.md`). New platform-specific code goes behind `soyeht-platform`
  or a `cfg` module with the same public shape on every system; where a feature
  cannot exist on a system it returns a typed `Unsupported` error.
  The merge gate lints `soyeht-platform`, `soyeht-process`, `soyeht-ptyd` and `soyeht-mcp`
  for `x86_64-pc-windows-msvc` when that target is installed (`rustup target add`), and
  `soyeht-engine` and `soyeht-cli` as well when the host also has a C compiler that targets
  Windows (their dependencies compile C); `scripts/check_engine_windows.ps1` checks all of
  it on Windows itself.

## Repository hygiene

- Write everything in the repository in English: code, comments, docs, commits
  and PRs.
- Add no AI attribution in commits, PR titles or PR bodies: no co-author
  trailers and no "generated by" lines. `forbidden_terms.py` rejects them.
- `scripts/check_repo_files.py` fails on archives, executables, logs and other binaries
  outside the directories it allows (fonts, icons, the PNGs of `docs/reports/`), on any file
  over 1 MB, and on such a file that any commit of `BASE..HEAD` added, even if a later
  commit removed it: it would stay in every clone. A kept exception goes in its
  `ALLOWED_FILES` with a reason.
- Never commit `benchmark/`. It is private local analysis and is ignored.
- Never commit secrets or machine-specific values: tokens, keys, team IDs, VM
  addresses or the restricted vocabulary. Pass them through the environment.
