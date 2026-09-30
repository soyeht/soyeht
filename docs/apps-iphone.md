# iPhone app in the Dev profile

Simulator build, matching CI:

```sh
bash scripts/check_ios.sh
```

To run in Simulator with Keychain access, use local signing:

```sh
SOYEHT_DEVELOPMENT_TEAM=YOUR_TEAM bash scripts/check_ios.sh --sign-simulator
```

The unsigned build serves CI compilation checks. It does not provide the
identity required by Keychain to run pairing.

Local device build with automatic development signing:

```sh
SOYEHT_DEVELOPMENT_TEAM=YOUR_TEAM bash scripts/check_ios.sh --sign
```

Open `apps/ios/SoyehtPhone.xcodeproj`, scheme `SoyehtPhone`, select your iPhone
and run with the team's development account. The bundle is
`com.soyeht.next.dev.ios`. The current app does not participate in this build.

On the new Mac app, choose “Add iPhone”. On the new iPhone app, choose “Scan Mac QR”.
Compare fingerprints and allow access on the Mac. Open a pane from the list.
A paired iPhone or iPad is a full remote control of the Mac (owner decision,
2026-09-26). Tap a terminal to type: the software keyboard has an Esc, Ctrl,
Tab and arrow row, and a hardware keyboard sends Return, Backspace, Tab, arrows,
Ctrl-C, Ctrl-D and Esc. For agent panes it also sends prompts, answers
permission requests and changes the harness's permission mode after engine confirmation. Both devices must be connected to the same tailnet.

On returning from the background, wait for full synchronization. If access was
revoked, pair again. “Forget this Mac” removes the iPhone's credential; revoking
on the Mac ends access at the engine and closes existing connections.

Without a compatible camera, the same link can be pasted or opened in the app.
The QR expires in ten minutes and only one approval consumes the code.
Never put the link, token or private key in a report or issue.
