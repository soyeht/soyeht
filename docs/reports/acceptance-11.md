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

Overall: acceptance 11 is **not passed** until the Mac-side failure in step 6
is fixed and the step is repeated.

## Defects found (handed to the executor)
1. **P1 · the Mac client's connection is closed when another client answers
   a permission**, and the Mac app then shows stale state as "Connected".
   Engine side: log every connection close with a reason and find the close
   path taken while publishing the answer events; e2e with two subscribed
   clients answering from the second. Client side: on READ_CLOSE show
   "disconnected" and reconnect with snapshot plus `events.subscribe {since}`.
2. The agent pane shows "Sign in to continue" while the session is
   authenticated and answering (`harness list` reports no auth methods).
3. The phone's pane view does not refresh when the app returns from the
   background while a request is pending; reopening the pane shows it.
4. The permission card shows the raw request JSON as its detail.
5. The transcript records four to five `usage` items per turn.

## Clean-up
Test directory, pane and harness entry removed after the executor's
inspection window; no other profile, service or harness entry was touched.
