# iPhone and iPad app in the Dev profile

One iOS app runs on iPhone and iPad (`TARGETED_DEVICE_FAMILY = 1,2`).

Simulator build, matching CI. The generic simulator destination builds the
binary both devices share, and the script fails if the built app stops
declaring the iPad family or its four orientations:

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
or iPad and run with the team's development account. The bundle is
`com.soyeht.next.dev.ios`. The current app does not participate in this build.

On the new Mac app, choose “Pair iPhone”; the same QR pairs an iPad. On the
device, choose “Scan Mac QR”. Compare fingerprints and allow access on the Mac.
Open a pane from the list.
A paired iPhone or iPad is a full remote control of the Mac (owner decision,
2026-09-26). Tap a terminal to type: the software keyboard has an Esc, Ctrl,
Tab and arrow row, and a hardware keyboard sends Return, Backspace, Tab, arrows,
Ctrl-C, Ctrl-D and Esc. For agent panes it also sends prompts, answers
permission requests and changes yolo after engine confirmation. Both devices must be connected to the same tailnet.

## Layout

At compact width (iPhone, or an iPad app in a narrow Split View or Stage
Manager window) the home screen is the list of workspaces and their panes, and
choosing a pane pushes it full screen. At regular width the same list becomes a
sidebar and the selected pane fills the detail column: agent conversation,
terminal with input, writing surfaces, or a web pane's “Open on the Mac”.
The detail starts on the pane focused on the Mac. When the shown pane closes,
the detail moves to the nearest pane in the same workspace, then anywhere; at
compact width it returns to the list instead. The app supports every iPad
orientation, Split View and Stage Manager.

## Hardware keyboard

A terminal takes the keyboard as soon as it is shown at regular width or while
a hardware keyboard is attached, so typing needs no tap first. The shortcuts
also appear in the iPad menu bar and when holding Command:

| Shortcut | Action |
| --- | --- |
| Command-] / Command-[ | Next or previous pane, across workspaces |
| Control-Command-S | Show and focus the sidebar |
| Command-1 … Command-9 | Jump to that workspace |
| Command-Return | Send the agent prompt (Return adds a line) |

## Access

On returning from the background, wait for full synchronization. If access was
revoked, pair again. “Forget this Mac” removes the device's credential; revoking
on the Mac ends access at the engine and closes existing connections.

Without a compatible camera, the same link can be pasted or opened in the app.
The QR expires in ten minutes and only one approval consumes the code.
Never put the link, token or private key in a report or issue.
