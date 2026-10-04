# Slice 4 · editor and usage manual procedure

This procedure verifies the native editor, a real language server, usage
readers, source-control review, agent review, and Markdown preview. Use only the
new Dev profile and a disposable Git repository. Never record a credential,
account identifier, usage value, usage percentage, or provider response body.

## Prepare the Dev profile

1. Install the pinned tools from [toolchains](toolchains.md). Confirm the
   supported usage providers are already signed in with their own tools.
2. From a dedicated worktree, run the local merge gate with the private
   restricted vocabulary supplied through `FORBIDDEN_TERMS`.
3. Stop only the new Dev services, build the workspace, and install it:

   ```sh
   launchctl bootout "gui/$(id -u)/com.soyeht.next.dev.engine" || true
   launchctl bootout "gui/$(id -u)/com.soyeht.next.dev.ptyd" || true
   cargo build --manifest-path engine/Cargo.toml --locked --workspace
   uv run python scripts/install_dev.py
   engine/target/debug/soyeht --profile dev status
   ```

4. Build and open the development-signed Mac app from the same tree:

   ```sh
   SOYEHT_DEVELOPMENT_TEAM=YOUR_TEAM bash scripts/check_mac.sh --sign
   open '.cache/mac-derived/Build/Products/Debug/Soyeht Dev.app'
   ```

5. Create a disposable Git repository containing a small Swift package, a
   committed Swift source file, a committed Markdown file, and a committed
   plain-text file for agent review.

## Exercise usage readers

1. Open the app bar and confirm one ring appears for every available reader.
2. Hover a ring and confirm the bounded detail balloon appears.
3. Run `usage refresh`, then `usage list` through the Dev CLI.
4. Record only each provider's state and source in
   [the on-device reader result](reports/usage-reader-ondevice.md).
5. Confirm a second refresh inside the provider deadline is rejected with the
   typed rate-limit result.

## Exercise the editor and language server

1. Open the committed Swift file in an editor pane. Confirm the file tree,
   breadcrumb, viewport, gutter, syntax colors, status bar, and companion pane.
2. Edit the function and add a declaration. Wait for the autosave deadline,
   then confirm the disk file, saved state, author, and history changed once.
3. Build the Swift package. Request language-server status, diagnostics, and
   symbols. Confirm the server is ready, the clean file has no diagnostics, and
   the symbol list contains the function and declaration.
4. Open the source-control diff. Confirm it compares `HEAD` with the current
   buffer, shows the changed lines, and keeps the request bound to the current
   editor revision.
5. Open history, preview an earlier version without replacing the live replica,
   then return to the current buffer.

## Exercise agent review and Markdown

1. Start the deterministic ACP test peer in the disposable workspace. Ask it
   to make two separated changes to the committed plain-text file.
2. Confirm the editor shows two review hunks, the author, and the live agent
   cursor. Accept each hunk in the running Mac app.
3. Reopen the file. Confirm both changes remain, the review is clear, the file
   is clean, and no unrelated line changed.
4. Open the Markdown file. Confirm the native preview renders only the bounded
   viewport and presents links as inert text.

## Record and clean up

Record the date, source commits, profile, schema, engine and ptyd generations,
language-server result, saved revisions, history count, diff summary, review
result, and Markdown result. Keep credentials, provider payloads, usage values,
bindings, and process environment out of the record.

Close the disposable panes. Restore the ordinary Dev harness registry, remove
the disposable test peer binary and repository, reinstall the ordinary Dev
engine, and confirm the engine reconnects to ptyd. Never stop or modify another
Soyeht profile.

## Recorded execution · 2026-09-13

The procedure ran against backend commit
`2d082d91f333189b5d724d25f682f699a7407cc0`, schema 15, engine generation 30,
and ptyd generation 29. The signed Mac client came from `c747a90`; the three
later commits changed only Dev launch configuration and ptyd inventory, so the
client tree was identical.

- All three real usage readers returned a usable reading. The separate reader
  report records only the allowed state and source facts.
- A real disposable Swift package opened in the native editor. Editing and
  autosave updated the file, `swift build` passed, and seven bounded history
  entries were available.
- The real Swift language server reached `ready`. Diagnostics returned no item
  after the source was corrected, and symbols returned the function and the
  declaration. The source-control view compared `HEAD` with the current buffer
  and showed two changed lines.
- The native Markdown preview rendered bounded content and left the example
  link inert.
- The deterministic ACP test peer produced two separated editor hunks. Both
  were accepted in the running app. Reopening the file showed revision 2,
  clean state, no conflict, and no remaining review.
- A production ACP adapter was also attempted, but its session did not reach
  `ready` in this run. The editor-review interaction therefore used the
  deterministic test peer. This remains a manual integration risk.
- The on-device run exposed an oversized supervisor inventory made of closed
  process records. PR #161 now returns only live processes while retaining
  closed receipts for idempotency. A real restart confirmed the engine could
  reconnect. Durable cleanup of old closed records remains follow-up work.

Cleanup completed with the ordinary harness registry restored, the test peer
removed, all slice-specific panes closed, the disposable repository deleted,
and the Dev engine reinstalled as generation 31 with ptyd generation 29.
