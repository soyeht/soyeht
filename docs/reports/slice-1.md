# Slice 1 · delivery and verification

Date: 2026-09-11. Profile: `Soyeht.next/dev`. License: Apache-2.0.

The delivery implements the protocol, supervisor, storage, engine, CLI and new
Mac and iPhone apps. All six automated acceptances passed. Manual acceptance is
[documented in a procedure](../manual-acceptance-slice-1.md); physical reboot,
a real camera and mobile-network reading still need to be exercised on real devices.
No tag or release was created.

## Delivered scope

The schema in `contracts/protocol.json` is the source of Rust, Swift and
TypeScript types. It contained 26 methods and 22 events when slice 1 closed.
After slice 2, the same generated schema contains 40 methods and 39 events,
with shared vectors, binary limits and numeric errors. Generation without a
diff is mandatory in CI.

`ptyd` keeps shells separate from the engine, with idempotent creation,
exclusive generation, 8 MiB retention and sequence acknowledgement. The engine uses
SQLite WAL with `synchronous=FULL`, 512 KiB segments, 32 MiB retention per pane
and replay by generation and offset. Gaps are declared.

The CLI opens, lists and closes panes and queries state and pairing. The Mac
app displays the terminal, sends keyboard input and resize, and allows iPhone
pairing or revocation. The iPhone reads the pane over WebSocket, stores its
identity in Keychain and fetches a full snapshot on returning from the background.
All clients use the engine protocol.

A full queue attempts to report `slow_consumer` and closes the connection.
Request logs contain an operation identifier, known method, result and duration,
without copying parameters or credentials. If the tailnet appears after login,
the engine detects the change and launchd restarts it; the supervisor preserves shells.

## Automated evidence

| Acceptance | Result | Verification |
| --- | --- | --- |
| 1 · reopen client | passed | Same pane and process; replay preserved. |
| 2 · kill engine | passed | SIGKILL and real launchd restart; same shell PID. |
| 3 · snapshot and events | passed | Changes during disconnection; cursor without loss or duplication. |
| 4 · close pane | passed | All clients remove it; restart does not resurrect it. |
| 5 · repeat creation | passed | One pane and one process for the same key. |
| 6 · overflow retention | passed | More than 8 MiB while the engine is stopped; replay declares `stream.gap`. |

`scripts/e2e.py` runs all six under its own temporary services. The final local
run passed in under ten seconds. The JSON report lives in `.cache/e2e.json`;
in CI it appears in logs and the run summary.

Fmt, clippy with warnings as errors, Rust tests, generator and guard tests,
Swift/TypeScript vectors and Mac/iOS builds passed. The seven native Swift tests
include a real shell, input, reconnection, HMAC pairing with consent, connection
signing, return with a fresh snapshot and revocation. The network test covers
late appearance, removal, address ordering and query errors.

CI for [PR 8](https://github.com/soyeht/soyeht/pull/8) passed with all components:
[run](https://github.com/soyeht/soyeht/actions/runs/34570456920). It took 4 min 14 s
from run creation to completion, including initial runner wait; the main job took
4 min 6 s. This is a measurement, not a guarantee about future queues.
The closing PR runs the same matrix against the final corrections.

## Applications and manual checks performed

The new Mac app was opened against a disposable engine. Keyboard input produced
real shell output and appeared in the terminal. The test found and fixed transfer
of focus to the terminal. The window and dimensions were checked.

Local Mac and iPhone builds passed with automatic development signing for the
configured team. CI builds both without signing. The iPhone opened in a signed
Simulator and queried Keychain without error. An unsigned build does not supply
the identity needed to execute that access.

The complete visual pairing procedure in Simulator was not finished: tap automation
could not complete the system dialog. Pairing, reading and revocation were exercised
by Swift clients against the real engine. Saving and restoring the credential through
the device UI, camera, reboot and mobile network remain pending manual acceptance.
They are not marked as passed because of builds or protocol tests.

## Run from scratch on a clean Mac

1. Install the tools pinned in [toolchains](../toolchains.md): Xcode,
   Rust, Python and Node. Configure the development account and tailnet.
2. Clone the private repository and create your worktree:

   ```sh
   git clone git@github.com:soyeht/soyeht.git ~/Documents/soyeht
   cd ~/Documents/soyeht
   git worktree add ../soyeht-dev -b dev/local origin/main
   cd ../soyeht-dev
   ```

3. Run the [README](../../README.md) verification commands. They build binaries
   before running native tests and acceptances.
4. Install the new profile and check the engine:

   ```sh
   uv run python scripts/install_dev.py
   engine/target/debug/soyeht --profile dev status
   engine/target/debug/soyeht --profile dev pane create --shell --cwd "$PWD"
   ```

5. Build the Mac app with development signing and open the result:

   ```sh
   SOYEHT_DEVELOPMENT_TEAM=YOUR_TEAM bash scripts/check_mac.sh --sign
   open '.cache/mac-derived/Build/Products/Debug/Soyeht Dev.app'
   ```

6. Open `apps/ios/SoyehtPhone.xcodeproj`, choose the `SoyehtPhone` scheme,
   iPhone and development team. Run. Follow
   [pairing](../apps-iphone.md) and the [manual procedure](../manual-acceptance-slice-1.md).

The installer refuses to replace a service that is still loaded. To update
only the engine while preserving shells, follow [local Dev](../dev-local.md).
Do not mix binaries from different commits during validation. To identify a
local build, record `git rev-parse HEAD` alongside test results.

## Decisions and known limits

- ADRs 0001–0002: PR-based delivery and protocol/identity format.
- ADRs 0003–0004: supervisor, sequence and transactions with segments.
- ADR 0005: identity of the merge produced by GitHub's service.
- ADRs 0006–0007: raw-shell state and safe input retries.
- ADRs 0008–0010: recoverable pairing, complete events and startup without
  depending on an external command.
- ADRs 0011–0013: isolated acceptances and native clients.
- ADRs 0014–0015: close reason, operation logs and tailnet after login.

Technical protection of main depends on the private repository's plan:
[issue 9](https://github.com/soyeht/soyeht/issues/9). The process rule remains
PRs with green checks, without direct pushes. Execution authorization allowed
autonomous merging with subsequent review. This is not technical protection.

Local commits use Caio Salgado's configured Git identity for both author and
committer. GitHub squash records the service's committer and the authenticated
account's author. The API refused the local email as an author option;
the account was not changed. This difference is recorded in ADR 0005 and was
not hidden by rewriting main.

Service log rotation still needs a disk limit:
[issue 10](https://github.com/soyeht/soyeht/issues/10). Terminal segment retention
already has its limit. The account's artifact quota prevented JSON upload;
the result remains in CI logs and the summary, without waiving any test.

A tailnet change causes clients to reconnect. A client that does not read may
miss even the full-queue error; the engine closes anyway. Durability depends
on storage honoring write acknowledgement. Reboot loses the shell process;
it restores state and recorded output with a new generation.

The Dev profile isolates names and data. It creates no additional operating-system
boundary for the user's shells. Agent panes, tasks, loops, ACP, push and the
Linux/Windows client are outside this slice, as defined by its scope.

## Delivery PRs

1. [Protocol and monorepo](https://github.com/soyeht/soyeht/pull/1).
2. [Supervisor](https://github.com/soyeht/soyeht/pull/2).
3. [Storage](https://github.com/soyeht/soyeht/pull/3).
4. [Engine](https://github.com/soyeht/soyeht/pull/4).
5. [CLI and Dev installation](https://github.com/soyeht/soyeht/pull/5).
6. [Acceptances under launchd](https://github.com/soyeht/soyeht/pull/6).
7. [Mac app](https://github.com/soyeht/soyeht/pull/7).
8. [iPhone app](https://github.com/soyeht/soyeht/pull/8).

The PR containing this report closes the connection fixes, procedure and usage
instructions. That PR's checks are the reference for completion.
