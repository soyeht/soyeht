# ADR 0066 · Harness catalogue and authentication boundaries

**Status:** Accepted · 2026-09-15

## Context

The engine currently treats the profile's `harnesses.json` as the complete harness catalogue. A new profile therefore has no choices, a macOS GUI process can miss executables installed outside its inherited `PATH`, and Preferences has no supported way to maintain custom entries. A pane may also publish authentication methods while its internal phase rejects `pane.auth.start`, leaving a visible sign-in action that cannot run. Terminal authentication currently appends login arguments to the normal ACP launch arguments even though ACP defines those login arguments as a replacement command line.

The public Agent Client Protocol registry publishes versioned launch metadata. It is useful as a source for a first-run catalogue, but startup, pane restore, and authentication must not depend on the network. User-authored launch commands remain local authority and must not acquire install behavior from remote metadata.

## Decision

The engine owns one merged harness catalogue. A pinned, compact snapshot derived from a reviewed Agent Client Protocol registry version ships with the engine and is available offline. The snapshot records its upstream version, source digest, agent identity, display name, version, and supported launch recipe. A refresh is explicit: it downloads a bounded document, validates the complete closed schema and pinned distribution rules, writes a new snapshot atomically, and never changes an active pane. No network read occurs during normal listing, restore, or launch.

`harnesses.json` becomes the bounded user-extension layer. Built-in and custom identifiers occupy separate namespaces; a custom entry cannot replace or weaken a built-in recipe. The engine exposes additive list/create/update/remove methods for custom entries. Writes are validated before an atomic file replacement. Custom entries carry only a display name, executable, arguments, and optional environment values. Identity variables such as `HOME`, `USER`, and the profile directory cannot be overridden. Custom entries never carry an install command, downloaded program, shell string, authentication secret, or registry refresh instruction. A custom entry bound to a live or restorable pane cannot be removed.

Installation status is derived at request time by resolving the launch executable. Resolution is deterministic and does not trust the user file as proof of installation. On macOS the engine obtains the user's launch environment once from the configured `SHELL`, using fixed login, interactive and command flags behind a bounded system-effect port. The initial allowlist is bash, fish and zsh; a missing, relative, unavailable or unsupported shell is a typed probe failure. The command has a short deadline, bounded output, strict decoding, no caller-provided shell text and no unbounded reader join. Failure uses the inherited environment for continued operation but remains a first-class check-setup fact; it cannot silently become a not-installed result. An explicit Preferences control and opening the new-pane composer invalidate this cache so a harness installed after app launch appears without restarting the app.

For a bare command the engine checks, in order, an explicit recipe override when present, the resolved interactive `PATH`, the inherited process `PATH`, `$HOME/.local/bin`, `/opt/homebrew/bin`, and bounded recipe-specific user directories declared by that catalogue recipe. It de-duplicates candidates, canonicalizes the result, and requires a regular executable file. Relative paths with separators are rejected. Package-runner recipes resolve their runner through the same policy and keep package versions pinned; no `latest` specifier or shell evaluation is allowed. A recipe may declare a minimum compatible local program version. A lower or unreadable version is check-setup with actionable copy, never a generic launch failure. The resolved executable and its digest remain the launch identity already checked by the supervisor.

Launchability and local installation are separate facts. Resolving `npx` or `uvx` proves only that the package runner can start; it does not prove that the person owns or has configured every package-backed catalogue agent. A built-in runner recipe counts as locally installed only after a successful native session or when bounded recipe metadata resolves a corresponding local product command. Direct binaries and explicit custom entries count as locally installed when their configured executable resolves. Runner-backed catalogue entries remain visible and selectable, but they cannot outrank locally evidenced entries or become the implicit default merely because a shared package runner exists.

The same resolved launch environment, including the interactive `PATH`, is passed to the harness process and to any capability probe. Discovery and execution therefore cannot disagree, and a script found through a version manager can still find its interpreter. Catalogue recipes execute directly and never replace `HOME`; an app-managed home is allowed only as a separately designed, explicit multi-account feature.

Harness status remains the engine's three-state result: unavailable when the executable cannot be resolved, sign-in when the ACP capability or an ACP `-32000` response requires authentication, and check-setup for a launch or probe failure that is neither absence nor authentication. The client renders this typed state and never infers readiness from an executable alone.

Built-in recipes inherit the user's real process environment and never replace `HOME`. Existing credentials therefore remain visible to the harness, and Soyeht does not present sign-in merely because it launched the program through an isolated test environment.

Authentication has one state transition and two bounded effect paths. Only an ACP `-32000` response moves the durable pane state to `NeedsLogin`; advertised authentication methods describe how to recover but do not by themselves mean that login is required. The state is committed before `pane.needs_login` is published. `pane.auth.start` accepts only a method from that published set. For an ACP `terminal` method, the client-visible terminal launches the same resolved program with the method arguments replacing the normal ACP arguments, waits for success, and then restarts the ACP session. For an ACP `agent` method, the engine sends `auth/login` through the live ACP session. Authentication arguments and environment are bounded; secrets remain owned by the harness process. Repeated starts use the existing idempotency receipt and cannot create two login effects.

Wire failures retain typed method and error information for programmatic clients. Product UI maps them once to concise user-facing copy; it never displays raw RPC method names or internal error-kind suffixes. A state race refreshes the pane and shows the current action rather than a blocking implementation-detail modal.

The Dev installer signs the engine, CLI, MCP bridge, and PTY daemon with stable bundle identifiers and the configured development signing identity before installing them into the profile. It verifies the resulting designated requirement and Team Identifier and fails closed when a stable identity was requested but cannot be established. Ad-hoc signing is not presented as persistent TCC identity.

Usage refresh has one coordinator per client profile. Automatic refresh, manual refresh, and reconnect requests share an in-flight operation, respect the engine's retry deadline, and schedule at most one later refresh. Views observe the shared reading; they do not start independent refresh loops.

## Consequences

- A fresh offline profile exposes the shipped catalogue, while local custom entries remain possible without hand-editing JSON.
- Installed state works from a sandboxed GUI environment whose inherited `PATH` omits common macOS install directories.
- Catalogue metadata never grants execution by itself; every launch still resolves and hashes a local executable.
- Authentication presentation and engine gating share the same state, and terminal login follows ACP argument replacement semantics.
- User-facing errors no longer leak wire method names or internal error kinds.
- Stable Dev code identity makes a TCC decision survive ordinary rebuilds when a development identity is configured.
- Existing pane, task, editor, browser, writing, usage, and protocol behavior remains additive; protocol version stays 1.
