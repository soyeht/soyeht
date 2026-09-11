# Slice 2 · acceptance 11 on a physical iPhone · 2026-09-11

Run by the reviewer against the Dev profile (`Soyeht.next/dev`), engine and
supervisor rebuilt from `main` 4478825 (schema version 6), macOS Darwin 25.4,
iPhone 13 mini on iOS 18 over mobile data (5G) with Tailscale, adapter
`@agentclientprotocol/claude-agent-acp@0.76.0` through `npx`. Mac app
`com.soyeht.next.dev.mac` and iPhone app `com.soyeht.next.dev.ios`, both
development-signed. Test directory `~/soyeht-acceptance-11` with the
`ask: ["Bash(*)"]` rule. No QR payload, token, key or account identifier is
recorded here. The iPhone was driven through XCUITest automation; the Mac app
through the accessibility API.

Identities: pane `7460a063-b743-4413-a731-e2a7c988c32e`, adapter process
33856, generation 2, model `default`; engine process 33491 before the kill
and 37811 after.

## Results

| Section | Result | Evidence |
|---|---|---|
| Prompt and history (steps 1–3) | pass | `Reply with exactly READY.` streamed `READY`; one accepted user message and one completed reply in the paginated transcript. |
| Pairing (step 4) | pass | Pairing link delivered to the phone app; the Mac consent dialog and the phone showed the same fingerprint (`bbd9…afd0`); after Allow, `device list` shows the device with that fingerprint; the phone showed the same pane, transcript, context use and controls as the Mac. |
| Mobile data (step 5) | pass | Wi‑Fi switched off in Settings (switch value 0, status bar on 5G); the engine kept an established TCP connection from the phone's tailnet address on port 8291. |
| Permission from the phone (step 6) | **partial** | The request appeared on the Mac and, after reopening the pane, on the phone. Allow once on the phone: `agent_requests.result` carries `{"by":"iphone","optionId":"allow-once"}`, the pane returned to `idle`, `permission-check.txt` contains exactly `SAFE`, the reply was `DONE`, and the card disappeared on the phone. **The Mac did not update**: it kept "Agent is working", the Yes/No card and the "Connected" indicator. Its socket to the engine received READ_CLOSE at 16:59:50.855, four seconds after the phone's answer; the engine logged no close, no slow consumer and did not restart. |
| Model change | pass (CLI) | `sonnet` applied with `sessionPreserved: true`, same adapter pid and generation; an invalid id was rejected with `invalid_params` and the session kept; restored to `default`. |
| Yolo | pass (CLI) | Yolo on: the shell prompt ran without a request, `yolo-check.txt` = `SAFE`; yolo off confirmed (`effectiveMode: ask`). |
| Engine recovery | pass (CLI) | SIGKILL of the engine during the 1…200 reply: launchd restarted it (37811); the pane kept adapter pid 33856 and generation 2; the reply completed with 200 lines; 3 user and 3 assistant messages, no duplicate. |

Overall: acceptance 11 is **passed**. The step-6 Mac-side failure (P1) was
fixed in PR 53 and re-verified on 2026-09-11; see the re-run section below.
Client defects 2 and 3 remain open and are tracked for a later slice; they do
not affect the acceptance criteria, but they did block live phone-tap
automation on the re-run (see the note there).

## Re-run after the P1 fix (PR 53) · 2026-09-11

Engine and supervisor rebuilt from `main` d34b7c7 (the PR 53 fix), Dev
profile, engine process 15272, adapter `@agentclientprotocol/claude-agent-acp@0.76.0`
process 15275, generation 3, same pane `7460a063-…`. Mac app rebuilt from the
same tree, process 16847, window `Soyeht · Dev`.

A fresh permission request for `printf SAFE > permission-check.txt` was
answered "allow once" from a second client while the Mac app stayed
subscribed to the engine. Result:

- The Mac app applied the answer: the Yes/No card cleared, the header stayed
  "Connected" and the footer "Dev profile · connected".
- Its socket to the engine did not close: zero READ_CLOSE in a ten-minute
  window spanning the answer, and both of the app's unix connections to
  `engine.sock` stayed open. The engine logged only ordinary `peer_closed`
  local connections (the reviewer's one-shot CLI calls), never the app's.
- `permission-check.txt` contains exactly `SAFE`, the pane returned to
  `idle`, and the receipt is `{"optionId":"allow-once","result":"selected"}`.

This exercises the exact P1 path: a permission answered by one client while
another subscribed client (the Mac app) receives the published answer event.
Under the old build that publish closed the Mac socket; under d34b7c7 it does
not. The P1 fix is confirmed.

Note on the answering client. The validated answer above was submitted from
the reviewer's CLI (`pane permission … allow`), which the engine records as a
second client and publishes to the Mac app exactly as a phone answer does.
The live phone tap could not be automated on this re-run: the iPhone app was
stuck on the "Sign in to continue" card with a running spinner (defects 2 and
3), and that continuous animation makes XCUITest's event injection and
screenshots hang indefinitely. The phone→engine answer path itself was
already proven end to end earlier the same day, when a phone "allow once"
recorded a receipt `{"by":"iphone","optionId":"allow-once"}` and wrote the
file; only the Mac-side close (now fixed) had failed then.

## Defects found (handed to the executor)
1. **FIXED (PR 53, verified 2026-09-11). P1 · the Mac client's connection is
   closed when another client answers a permission**, and the Mac app then
   shows stale state as "Connected". Engine side: log every connection close
   with a reason and find the close path taken while publishing the answer
   events; e2e with two subscribed clients answering from the second. Client
   side: on READ_CLOSE show "disconnected" and reconnect with snapshot plus
   `events.subscribe {since}`.
2. The agent pane shows "Sign in to continue" while the session is
   authenticated and answering (`harness list` reports no auth methods).
3. The phone's pane view does not refresh when the app returns from the
   background while a request is pending; reopening the pane shows it.
4. The permission card shows the raw request JSON as its detail.
5. The transcript records four to five `usage` items per turn.

## Clean-up
Test directory, pane and harness entry removed after the executor's
inspection window; no other profile, service or harness entry was touched.
