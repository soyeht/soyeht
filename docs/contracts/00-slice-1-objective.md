# Objective · New Soyeht slice 1 (autonomous execution to completion)

This document is self-contained. The executor must not ask Caio questions:
every ambiguity becomes a decision recorded in `docs/adr/NNNN-*.md`
(one page: context, decision, rationale), and work continues. Standard:
“this is a professional company; I want professional code; never consider
sloppy stuff”. Date: 2026-09-10.

## 0. Absolute safeguards
1. NEVER touch `~/Documents/SwiftProjects/iSoyehtTerm`,
   `~/Documents/theyos`, production data
   (`~/Library/Application Support/Soyeht`, `Soyeht Dev`, `SoyehtDev*`),
   or LaunchAgents `com.soyeht.engine` / `com.soyeht.ptyd`. These are the
   product Caio uses NOW. Read-only, as reference.
2. Everything in the new product lives in the new repository and uses its
   own names: data in `~/Library/Application Support/Soyeht.next/dev/`,
   LaunchAgents `com.soyeht.next.dev.engine` and `com.soyeht.next.dev.ptyd`,
   port 8291, bundle IDs `com.soyeht.next.dev.*`. This is the **Dev profile**:
   Caio continues using production while agents test here.
3. No secrets in the repository. No tags or releases. Publish nothing
   outside this repository's GitHub.
4. Contracts in `~/Documents/soyeht/docs/contracts/` (01–08, 00-slice-1) are
   the source of truth for BEHAVIOR. Where code and contract disagree,
   the code is wrong. If a contract is impossible to fulfill, record an
   explanatory ADR and stop at that point with a report.
5. Do not copy code from the current product without review; proven ideas
   may be rewritten (PTY segment format, HMAC pairing, APNs), citing their
   source in an ADR. Do not copy AGPL or proprietary repository code;
   MIT/Apache with attribution if anything is copied.

## 1. Delivery
GitHub repository `soyeht/soyeht` (created; private; Apache-2.0), monorepo:
```
contracts/            protocol.json (single source) + test vectors + generator
engine/               Cargo workspace: soyeht-engine, soyeht-ptyd, soyeht-cli, soyeht-protocol (generated types), soyeht-store
apps/mac/             Xcode: Swift app (SwiftUI/AppKit) for macOS, Dev target
apps/ios/             Xcode: Swift app for iPhone, Dev target (may be one project with two targets)
apps/desktop/         (empty in this slice; TypeScript, Linux/Windows, later)
design/               soyeht-desktop.pen (copy from ~/Documents/soyeht/design/)
docs/contracts/       versioned contracts 01–08 and 00-slice-1 (internal analysis NEVER enters the repository)
docs/adr/             decisions
.github/workflows/    CI
```
Pinned toolchains: `rust-toolchain.toml` (current stable), Xcode/Swift version
recorded in `docs/toolchains.md`, committed `Cargo.lock`.

## 2. What to build (exact scope, nothing beyond)
1. **`contracts/protocol.json`**: JSON Schema (explicit, documented subset)
   with ALL methods, events, errors and binary headers from contract 08
   used in this slice: `auth.challenge`, `hello`, `welcome`, `state.snapshot`
   (+ `state.snapshot_part`), `events.subscribe`, `workspace.*`,
   `pane.create|close|rename|focus|resize|input|stream.subscribe|stream.unsubscribe`,
   `pane.record.set`, `device.pair.start|decide|list|revoke`,
   `pair.begin|prove|result`, `engine.status`, `settings.get|set`;
   events `workspace.*`, `pane.created|closed|renamed|state_changed`,
   `stream.replay_start|replay_done|gap`, `device.pair_requested|paired|revoked`,
   `engine.updating|restarted`, `ptyd.reconnected|lost`; errors from 08 §7;
   limits from 08 §8. Type generator for Rust (serde), Swift (Codable) and
   TypeScript, with round-trip tests per message and binary vector.
   CI fails if generated ≠ committed.
2. **`soyeht-ptyd`**: contract 07 process supervisor (PTY only in this slice;
   the interface already anticipates pipes): file registry, idempotent
   `instanceKey`, 8 MiB retention with `gap`, `seq`/`ack`, engine generation
   rejecting lower generations, its own versioned socket. Dev LaunchAgent
   with KeepAlive.
3. **`soyeht-engine`**: launchd KeepAlive; single SQLite database (`user_version`,
   numbered migrations, WAL, `synchronous=FULL`); workspace and pane state;
   screen segments (512 KiB, 32 MiB per pane, replay by offset, `gap` record);
   adoption of `ptyd` on startup (reconcile before marking a pane dead);
   protocol server on Unix socket (`0700`/`0600`) and plaintext WebSocket
   on Tailscale addresses + loopback (never `0.0.0.0`; LAN only with the
   pairing window open); `auth.challenge → hello → welcome` handshake;
   QR pairing (08 §2.2) with Ed25519/HMAC-SHA-256 and consent; multipart
   snapshot + cursor-based events in the same transaction; mandatory,
   persisted `idempotencyKey`; 8 MiB output queue with `slow_consumer`;
   pane states `iniciando|trabalhando|ociosa|morta|recuperando` derived only
   from process facts (no hooks or regex in this slice); structured logs
   without secrets, with operation IDs.
4. **`soyeht-cli`**: `soyeht --profile dev status | pane create --shell |
   pane list | pane close <id> | device pair-start | device list`.
5. **Mac app (Swift)**: window with pane list and a terminal (native xterm
   in Swift; choose an MIT/Apache library and record an ADR; design follows
   `design/soyeht-desktop.pen`, neomorphic style, Screen 1); local-socket
   connection; `snapshot + subscribe`; keyboard input and resize;
   “Add iPhone” button displaying QR; pairing consent dialog.
6. **iPhone app (Swift)**: scans QR, pairs, connects through Tailscale,
   displays pane list and pane screen (read-only); reconnects through
   `hello → snapshot → subscribe` on returning from the background;
   `Info.plist` with `NSAllowsArbitraryLoads` and explanatory comment (contract 06).
7. **CI (GitHub Actions)**, blocking every PR: `cargo fmt --check`,
   `cargo clippy --all-targets -D warnings`, `cargo test`, protocol generation
   without a diff, Mac and iOS app builds (xcodebuild, without distribution
   signing), and the end-to-end test below on a macOS runner. Protected `main`:
   mandatory PR, 1 approval, required checks. Conventional Commits.

## 3. Acceptance (work ends only when all 7 pass, automated where marked)
1. [CI] Close and reopen the client (simulated by CLI/test): pane remains, replay without a gap.
2. [CI] `kill -9` the engine + launchd restart: shell with the SAME PID; engine adopts through `ptyd`; screen continues; client reconnects and sees the same content.
3. [CI] Disconnect/reconnect client: `hello → snapshot → subscribe`, no lost or repeated event (test injects changes during disconnection).
4. [CI] Close pane: disappears for every client; engine restart does not resurrect it.
5. [CI] Repeat `pane.create` with the same `idempotencyKey`: just one pane.
6. [CI] Fill `ptyd` retention with the engine stopped: on return, `stream.gap` event; never false continuity.
7. [manual, documented procedure] Restart the Mac: old shell dies; layout, cwd and recorded screen return; pane marked “new shell”. Also: real iPhone QR pairing and pane reading with Wi-Fi disabled (Tailscale).

## 4. How to work
- Order: contract/schema → ptyd → engine → CLI → e2e tests → Mac app →
  iPhone app → manual procedure. Each step is a small PR with green CI.
- Every decision not settled by a contract becomes an ADR; do not ask.
- No “for now”: no deferred TODO without an open issue.
- On completion: report in `docs/reports/slice-1.md` describing what passed,
  what did not and why, decisions (ADRs), how to run from scratch on a clean
  Mac, and known risks. Send a summary to victoria (who reviews against
  the contracts) with PR links.

## 5. Additional safeguard (Caio, 2026-09-10): competitor names NEVER in the repository
Writing another competing product or company name in any versioned file
(code, docs, ADR, comment, commit message, PR) is prohibited. Integrations
used by the product (Claude Code, Codex, Gemini, Cursor CLI, Tailscale, GitHub,
ACP, worktrunk) may be cited. Implement a blocking `forbidden-terms` CI check
in the first PR: the term list does NOT exist in the repository in any form
(not even hashes: short words are guessable). It lives in a repository secret
(`FORBIDDEN_TERMS`, one term per line, maintained by victoria); the script
reads it from the environment, tokenizes all text files (lowercase; tokens
and consecutive pairs) and PR commit messages, and fails on a match.
The failure message shows only file and line, never the term, so the CI log
cannot leak it either. If the secret is unavailable (fork PR), the check
fails closed. Cite the source of an idea as “internal analysis”.
