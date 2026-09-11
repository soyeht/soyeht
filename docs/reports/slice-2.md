# Slice 2 · agent panes over ACP

Date: 2026-09-11. Profile: `Soyeht.next/dev`. License: Apache-2.0.

The delivery adds native agent panes over ACP v1 to the protocol, process
supervisor, storage, engine, CLI, Mac app and iPhone app. All ten deterministic
acceptances passed under disposable launchd services. The authenticated adapter
check passed all four automated flows. Physical iPhone verification over mobile
data is [documented as a procedure](../manual-acceptance-slice-2.md) and remains
for Caio to run on the device. No tag or release was created.

## Delivered scope

The shared schema now defines 40 methods, 39 events and 20 numeric error kinds.
Agent panes include prompt submission, a durable queue, cancellation, permission
and elicitation answers, confirmed configuration, yolo, sign-in, paginated
transcript, draft presence, harness discovery and model discovery. Rust, Swift
and TypeScript types and vectors are generated from that schema.

`soyeht-ptyd` supervises ACP processes through pipes. Standard output carries
ACP JSON-RPC records, standard error goes to a private profile log, and input is
deduplicated. A process keeps its instance key, generation, sequence offsets,
unacknowledged output and unanswered-request ledger while the engine is absent.
A private guardian terminates the process group when the supervisor dies.

`soyeht-acp` implements the ACP v1 client boundary. It initializes the native
session, correlates JSON-RPC records, normalizes updates, confirms configuration,
handles native filesystem and terminal requests, and uses the prompt response as
the end of a v1 turn. Unsupported protocol versions are refused. The deterministic
native test peer covers streaming, permissions, forms, configuration, cancellation,
load, sign-in and deliberate process loss without contacting a model service.

The engine persists agent checkpoints, exact native outbox records, open native
requests, normalized transcript records, capability probes, attention items and
private load staging. Effects and their public events commit in one transaction.
The first valid answer to a permission or elicitation wins. Late answers return
the stored result and never send a second native response.

Accepted prompts have stable identities and origins. Waiting prompts survive an
engine restart, respect the 64-item and 64-KiB limits, and do not dispatch an
automatic prompt while a human draft is active. Cancellation pauses the queue
until an explicit resume. Transcript pages contain at most 200 records and 512
KiB, with 32 MiB retained per pane.

Process recovery distinguishes engine loss from supervisor loss. The engine
adopts a live ACP process without creating another session. After supervisor
loss, it starts a new process and attempts `session/load` only when a disposable
probe proved support for that fingerprint. Failed or unsupported load creates a
new session and a visible `pane.session_lost` notice. It never silently claims
continuity.

The CLI creates and controls agent panes, lists native catalogs, pages queues and
transcripts, answers a selected permission, and starts terminal sign-in. Every
mutation supports an idempotency key. A retry with the same key and different
arguments is rejected.

The Mac app creates terminal or agent panes and renders normalized messages,
thoughts, tool calls, plans, inline terminal output, questions, queue state,
context usage and confirmed controls. The iPhone renders the same agent state,
sends prompts, answers permissions and changes yolo. Both clients reconstruct
agent state from a snapshot and subsequent durable events.

## Acceptance evidence

| Acceptance | Result | Verification |
| --- | --- | --- |
| 1 · two clients and history | passed | Two live protocol clients received identical streamed chunks. Accepted user and agent messages survived restart and paginated without duplicate identities. |
| 2 · permission | passed | Both clients saw the same request. The first answer won, a later answer received that result, and accelerated expiry produced a durable attention item. |
| 3 · cancellation | passed | A slow turn returned to `idle` within ten seconds, kept the healthy process and paused the waiting queue. |
| 4 · engine loss | passed | A request created while the engine was absent was recovered from ptyd, answered once and completed on the same native PID. |
| 5 · model and effort | passed | Native confirmation emitted changing and changed events with `sessionPreserved: true`. A rejected value kept the session. |
| 6 · yolo | passed | Native confirmation enabled and disabled yolo. It hid and approved a permission, while a separate elicitation remained visible and unanswered. |
| 7 · queue and draft | passed | A busy prompt waited and ran after idle. An automatic prompt stayed durable across restart until the human draft cleared. |
| 8 · ptyd loss | passed | Proven load preserved the native session and ordered transcript. Unsupported load created another session with a visible loss notice. |
| 9 · capability probe | passed | False load advertising was recorded as unsupported, reused for the same fingerprint and probed again only after the version changed. |
| 10 · sign-in | passed | Authentication required a visible state, opened a separate raw terminal, published completion and then opened the agent session. |
| 11 · authenticated adapter and device | partial manual | The disposable adapter check passed permission, model change, yolo and engine adoption. The physical iPhone and mobile-data steps are written but were not run during implementation. |

`scripts/e2e_agents.py` runs acceptances 1–10 with uniquely named launchd jobs,
temporary data and the deterministic native peer. It removes all processes and
files after the run. `scripts/e2e_real_acp.py` is opt-in and uses a pinned,
authenticated official Claude Code ACP adapter in another disposable profile.
Its final run passed these four checks:

1. a real prompt and one harmless permission answered through a second client;
2. a model change and restoration without replacing the session;
3. a confirmed yolo round trip with an automatically handled permission; and
4. engine termination during a turn followed by adoption of the same process.

The real compatibility check does not modify the installed Dev profile. It
installs the adapter and a temporary project rule under a random directory,
uses harmless file writes, and removes the directory and launchd jobs afterward.

## Local merge gate

Hosted GitHub Actions is disabled by project decision. The required merge gate
is `scripts/check_all.sh`, supplied with the private restricted vocabulary and
the exact PR base and final payload. The final implementation run passed:

- the restricted-vocabulary scan over files, commit messages and PR text;
- protocol generation without a diff, generator tests and shared vectors;
- Rust formatting, clippy with warnings as errors, build and workspace tests;
- six shell and ten agent acceptances under disposable launchd services;
- four authenticated adapter compatibility checks;
- Swift protocol and shared-client tests plus strict formatting;
- unsigned Mac and iOS Simulator builds; and
- TypeScript checks and tests.

The workflows remain in the repository for later reactivation. A queued hosted
run is not accepted as evidence while hosted execution is disabled.

## Run from scratch on a clean Mac

1. Install the pinned tools from [toolchains](../toolchains.md). Configure the
   development account and tailnet needed for signed device work.
2. Clone the repository and create a dedicated worktree:

   ```sh
   git clone git@github.com:soyeht/soyeht.git ~/Documents/soyeht
   cd ~/Documents/soyeht
   git worktree add ../soyeht-dev -b dev/local origin/main
   cd ../soyeht-dev
   ```

3. Provide the private vocabulary through `FORBIDDEN_TERMS` and run
   `scripts/check_all.sh`. Add `SOYEHT_REAL_ACP=1` only when local authentication
   is available and the pinned adapter compatibility check is intended.
4. Build and install the new Dev services:

   ```sh
   cargo build --manifest-path engine/Cargo.toml --locked --workspace
   python3 scripts/install_dev.py
   engine/target/debug/soyeht --profile dev status
   ```

5. Add a harness entry to the Dev profile as described in
   [the agent CLI guide](../agent-cli.md). Keep authentication material out of
   the registry. Open an agent pane and send a prompt from the CLI.
6. Build the Mac app with development signing:

   ```sh
   SOYEHT_DEVELOPMENT_TEAM=YOUR_TEAM bash scripts/check_mac.sh --sign
   open '.cache/mac-derived/Build/Products/Debug/Soyeht Dev.app'
   ```

7. Open `apps/ios/SoyehtPhone.xcodeproj`, select the `SoyehtPhone` scheme, a
   physical iPhone and the development team. Pair through the Mac consent flow.
8. Follow the [slice 2 device procedure](../manual-acceptance-slice-2.md). Record
   only the requested versions and process/session identities. Never record the
   pairing payload, device credential or provider credential.

The installer and test scripts accept only the new Dev profile or a disposable
data directory. Replacing only the engine preserves supervised processes;
replacing ptyd terminates them and exercises native load on recovery.

## Decisions and known limits

- ADR 0016 defines the bounded public agent-pane contract and ACP v1 turn end.
- ADR 0017 defines supervised pipes, process guardians and unanswered-request
  recovery.
- ADR 0018 records the canonical English wire values and SQLite migration.
- ADR 0019 defines the ACP client boundary, fingerprint probes and deterministic
  peer.
- ADR 0020 defines transactional agent state, outbox, requests and transcript.
- ADR 0021 keeps prompt bodies out of the bounded pane snapshot.
- ADR 0022 publishes loaded native history atomically from private staging.
- ADR 0023 recovers requests before acknowledging lost raw output.
- ADR 0024 pauses the durable queue after interruption.
- ADR 0025 bounds native metadata and preserves uncertain operations.
- ADR 0026 preserves authentication state across snapshots and requires a proven
  native operation before enabling steering or compaction.

The physical iPhone flow over mobile data is still pending. Until it passes, the
report claims protocol, native-client and authenticated-adapter evidence, not
physical-device acceptance.

ACP v1 has no standard steering or context-compaction operation. The apps show
the unavailable steering action with its required explanation and do not offer
compaction. A later adapter may enable either control only after a disposable
per-fingerprint probe proves its recovery behavior.

Native process guardians control cooperative descendants in their process group;
they do not isolate a program that deliberately detaches. Service diagnostic-log
rotation remains tracked by [issue 10](https://github.com/soyeht/soyeht/issues/10).

The optional authenticated check downloads an exact adapter version and depends
on existing local authentication. Its deterministic safety comes from the
disposable profile, temporary working directory and harmless prompts; it is not
a substitute for the in-repository native peer.

Tasks, loops, a lead agent and the Soyeht MCP session service remain outside this
slice. Continuation packages and harness switching remain slice 4. ACP v2 is
refused until a separately reviewed implementation exists.

## Delivery PRs

1. [Agent protocol](https://github.com/soyeht/soyeht/pull/13).
2. [English documentation](https://github.com/soyeht/soyeht/pull/19).
3. [Canonical wire values and migration](https://github.com/soyeht/soyeht/pull/20).
4. [ACP process supervision](https://github.com/soyeht/soyeht/pull/21).
5. [ACP v1 client and deterministic peer](https://github.com/soyeht/soyeht/pull/22).
6. [Agent persistence](https://github.com/soyeht/soyeht/pull/25).
7. [Required local merge gate](https://github.com/soyeht/soyeht/pull/31).
8. [Agent engine runtime](https://github.com/soyeht/soyeht/pull/33).
9. [Agent CLI](https://github.com/soyeht/soyeht/pull/34).
10. [Launchd agent acceptances](https://github.com/soyeht/soyeht/pull/35).
11. [Mac and iPhone agent panes](https://github.com/soyeht/soyeht/pull/36).
12. [Snapshot continuity and native-control boundaries](https://github.com/soyeht/soyeht/pull/38).
13. [Authenticated adapter and device procedure](https://github.com/soyeht/soyeht/pull/41).

The PR containing this report records the final audit and closes slice 2.
