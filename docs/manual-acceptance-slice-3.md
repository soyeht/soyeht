# Slice 3 · manual task workflow

This procedure verifies the workspace board, a real ACP task executor, the
contract-drift reviewer, human moves and the per-pane loop. Use only the new Dev
profile and a disposable Git repository. Do not point a task at personal work.

## Prepare the Dev profile

1. Install the pinned tools from [toolchains](toolchains.md). Install worktrunk
   and the official Claude Code ACP adapter described in
   [the agent CLI guide](agent-cli.md).
2. From a dedicated repository worktree, run the local merge gate with the
   private restricted vocabulary supplied through `FORBIDDEN_TERMS`.
3. Stop only the new Dev services, build the workspace and install it:

   ```sh
   launchctl bootout "gui/$(id -u)/com.soyeht.next.dev.engine" || true
   launchctl bootout "gui/$(id -u)/com.soyeht.next.dev.ptyd" || true
   cargo build --manifest-path engine/Cargo.toml --locked --workspace
   python3 scripts/install_dev.py
   engine/target/debug/soyeht --profile dev status
   ```

4. On first use, macOS may ask whether `soyeht-ptyd` may access the Documents
   folder because worktrunk creates task worktrees beside a repository there.
   Bring the system dialog to the front and choose **Allow**. A denied or
   unanswered dialog leaves native session creation waiting; it is not an ACP
   authentication prompt.
5. Build and open the development-signed Mac app from the same commit:

   ```sh
   SOYEHT_DEVELOPMENT_TEAM=YOUR_TEAM bash scripts/check_mac.sh --sign
   open '.cache/mac-derived/Build/Products/Debug/Soyeht Dev.app'
   ```

6. Create a disposable Git repository with one committed text file. Open one
   shell pane there so the Mac app creates the workspace. Record the commit,
   workspace ID, engine generation and adapter version. Do not record bindings,
   credentials or pairing data.

## Create, dispatch and review a task

1. Open **Tasks**, select **Board**, and choose **New task**.
2. Fill title, description, **MUST NOT CHANGE**, verification, executor,
   permission and priority. Leave **MUST NOT CHANGE** empty once and verify both
   create buttons remain disabled. Then fill it and choose **Create and
   dispatch**.
3. Verify one task card and one executor pane appear. The card must show its
   `WS-n` ID, priority, worktree branch and engine-published phrase.
4. In the task detail, watch the real agent create at least two subitems before
   doing work, add a milestone comment, finish the subitems and request review.
5. Verify a separate `Review WS-n` pane opens. Its result must appear as a task
   comment. A returned result keeps the same attempt open and sends the reason
   back to the executor. An approved result moves the task to **Completed** and
   clears the owner.
6. For a no-public-surface change, the reviewer must return exactly an empty
   finding list, either as bare JSON or as one JSON code fence. Extra prose,
   another fence language or an invented file-level surface must fail closed.

## Move cards

1. Dispatch a harmless task, then drag it from **In progress** to **To do**.
   Verify the active turn stops, the attempt closes as `returned`, the owner is
   cleared and history records the person, states and reason.
2. Drag the same **To do** card to **Completed**. Verify the app shows the
   engine's `transition_refused` reason and the card remains in **To do**.
3. Drag or move that card to **Cancelled**. Verify the fifth column appears only
   after it contains the card.

## Run a loop through an engine restart

1. Create one harmless `todo` task configured for the same harness. Its prompt
   must tell the executor to create subitems, comment, request review and call
   `task_complete` with both drift declaration lists. It may be a no-op that
   changes no file.
2. Open an agent pane in the workspace and send one short prompt so the pane is
   `idle` and its MCP server is connected.
3. Restart only the Dev engine. Keep ptyd and the native agent process alive:

   ```sh
   launchctl bootout "gui/$(id -u)/com.soyeht.next.dev.engine"
   python3 scripts/install_dev.py --component engine
   engine/target/debug/soyeht --profile dev status
   ```

4. Enable the loop, replacing the example pane ID:

   ```sh
   engine/target/debug/soyeht --profile dev loop set PANE_ID \
     'Use the bound task_next tool to pull the next available task. If task_next returns empty, reply NOTHING TO DO.' \
     5 8 three_empty true --idempotency-key UNIQUE_KEY
   ```

5. Verify the pre-restart MCP server reconnects and `task_next` returns
   `willRestartSession: true`. The same pane must gain a new session generation,
   use the task worktree as its directory and receive the full task prompt.
6. Let the executor and reviewer complete the task. Verify the loop then runs
   three more turns, each `task_next` result is `empty: true`, and the engine
   publishes `loop.stopped` with the pane left `idle`.

## Record and clean up

Record the date, source commit, profile, engine and ptyd generations, adapter
version, task IDs, attempt numbers, accepted and refused moves, review outcome,
loop rounds and stop reason. Keep credentials, bindings and prompt service data
out of the record.

Disable any loop still enabled and close every disposable pane. Restore the Dev
harness registry if it was changed. Remove only the disposable task worktrees
after their processes have stopped. Reinstall the ordinary Dev launchd files if
the test changed them. Never stop or modify another Soyeht profile.

## Recorded execution · 2026-09-12

The procedure passed on commit `aded701c01e7b4d7579e48a8b480e098bf3d374c`
with the official adapter version `0.76.0`.

- `WS-1` was created and dispatched from the Mac board. Its successful real
  executor run created two subitems, changed only the allowed fixture, added a
  milestone comment and requested review. A reviewer first returned a malformed
  zero-surface answer. After the fail-closed prompt/parser correction, attempt
  12 reused the worktree and the reviewer approved it as `completed`.
- `WS-2` proved board moves. `in_progress → todo` closed the active attempt and
  cleared the owner. `todo → completed` displayed `transition_refused`. The
  probe was then moved to `cancelled`.
- `WS-3` was pulled by a loop after engine generation 14 restarted as generation
  15. The same pane changed from session generation 1 to 2, completed two
  subitems, two comments and one approved review in attempt 1. It then recorded
  three distinct loop prompts and three `empty: true` results before stopping
  `idle`.

During preparation, two queued macOS Documents permission dialogs explained
the earlier native-session waits. Allowing them made launchd execution succeed.
After recording the evidence, all disposable panes and directories were
removed, the original harness and local adapter configuration were restored,
and the normal Dev launchd files were reinstalled. The final clean service
state was engine generation 16 and ptyd generation 21 with no open pane.
