# Slice 2 · real ACP and device procedure

This procedure covers slice 2 acceptance 11. The automated suite covers the ten
deterministic acceptances with the native test peer. The optional compatibility
check covers the authenticated adapter without using the installed Dev profile.
Only the steps below establish behavior on a physical iPhone over mobile data.

## Run the disposable compatibility check

Build the Rust workspace and run the opt-in check from the repository root:

```sh
(cd engine && cargo build --locked --workspace)
SOYEHT_REAL_ACP=1 uv run python scripts/e2e_real_acp.py \
  --report .cache/e2e-real-acp.json
```

The check installs the Claude and Codex adapter versions the registry snapshot
pins under a temporary directory. For each it opens an authenticated session,
switches between the adapter's own permission modes, answers a harmless
permission, changes the model without replacing the session, and kills only its
randomly named disposable engine during a turn. A pass does not establish the
physical-device steps below.

## Configure only the new Dev profile

Follow [Dev installation](dev-local.md), [the Mac app](apps-mac.md), and
[the iPhone app](apps-iphone.md). Confirm that the two service labels begin with
`com.soyeht.next.dev.` and that their data directory is exactly
`~/Library/Application Support/Soyeht.next/dev/`.

The Dev harness registry accepts an absolute executable plus arguments. Locate
`npx` and merge this entry into the `harnesses.json` array in that Dev directory.
Preserve every existing entry. Do not place credentials in the file.

```json
{
  "id": "claude-code",
  "name": "Claude Code",
  "command": "/opt/homebrew/bin/npx",
  "args": ["--yes", "@agentclientprotocol/claude-agent-acp@0.76.0"],
  "version": "0.76.0"
}
```

Use the actual absolute path returned by `command -v npx` when it differs. The
adapter uses the existing local authentication. Do not copy tokens into the
registry, evidence, logs, or a test workspace. Run `soyeht --profile dev harness
list` and confirm that the entry is visible before opening a pane.

Create a new empty test directory outside every source repository. In that
directory, create `.claude/settings.local.json` with the following test-only rule.
It makes the harmless shell action in the next section request permission even
when the user's broader settings already allow it.

```json
{
  "permissions": {
    "ask": ["Bash(*)"]
  }
}
```

Do not add this file to a source repository. Delete the whole test directory at
the end.

## Prompt and answer from the iPhone

1. In the new Mac app, create an agent pane with harness “Claude Code”, mode
   “Manual”, and the empty test directory as its working directory.
2. Wait for the pane to become ready. Record the pane ID, session ID, process ID,
   generation, adapter version and current model. Do not record authentication
   data or the contents of the pairing link.
3. Send: `Reply with exactly READY.` Confirm that the response streams and that
   Mac history contains one accepted user message and one completed response.
4. Pair the new iPhone app through the Mac consent flow. Open the same agent pane.
   Confirm that its transcript and current controls match the Mac.
5. Disable Wi-Fi on the iPhone. Keep mobile data and Tailscale enabled. Return to
   the pane and wait until it reports a live connection.
6. From either client send: `Use the shell to run: printf SAFE >
   permission-check.txt. Then reply DONE.` Confirm that the request appears on
   both clients. Allow it once on the iPhone. The Mac must show that the answer
   came from the phone, the request must disappear on both clients, and the file
   must contain exactly `SAFE`.

## Model, permission mode and engine recovery

1. On the Mac, select another model advertised by the live session. Keep the
   session ID visible. The control must show an applying state, then the confirmed
   model. The session ID and process ID must stay unchanged. Restore the original
   model and confirm it in the same way.
2. Choose “Bypass permissions” in the pane's mode menu. Send: `Use the shell to
   run: printf SAFE > full-access-check.txt. Then reply DONE.` The file must
   contain exactly `SAFE`, and neither client may show a permission request.
   Choose “Manual” again and wait for the menu to confirm it before proceeding.
3. Send: `Write the integers 1 through 200, one per line, and nothing else.` As
   soon as output begins, run this exact command in a separate terminal:

   ```sh
   launchctl kill SIGKILL "gui/$(id -u)/com.soyeht.next.dev.engine"
   ```

4. Confirm that launchd starts the Dev engine again. The pane must reconnect to
   the same adapter process ID and session ID, continue receiving output, end the
   turn once, and return to `idle`. Mac and iPhone transcripts must converge after
   snapshot and event replay, without a duplicate user message or completed reply.

## Record and clean up

Record the date, commit, macOS and iOS versions, adapter version, pane/session
identities before and after engine recovery, and pass/fail for each section. Never
record a QR payload, device token, private key, account identifier, or provider
credential. If a step fails, retain only operation IDs from the new Dev logs and
open a fix before marking acceptance 11 passed.

Close the test pane, remove its empty test directory, and remove the test harness
entry if it is no longer needed. Do not delete another harness entry, another
profile's data, or any service whose label does not begin with
`com.soyeht.next.dev.`.
