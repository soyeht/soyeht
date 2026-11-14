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

## Store build

The project builds the Dev app by default. A store build changes two build settings and nothing
else, as the Mac packager does (ADR 0120): `SOYEHT_APP_BUNDLE_ID=com.soyeht.next.ios` (the
production identifier, ADR 0112) and `SOYEHT_PRODUCT_NAME="Soyeht Next"`, the production Mac app's name (`displayNames.macApp` in
`contracts/profiles.json`). The version is
`MARKETING_VERSION` and the build number is `CURRENT_PROJECT_VERSION`, both written by
`uv run python scripts/release.py bump ...` from `contracts/release.json` (ADR 0115); never type
them. Signing credentials come from the environment, never from the repository.

`scripts/package_ios.py` does the whole build (ADR 0120's pattern, as `package_mac.py` does for
the Mac) and stops before the upload:

```sh
export SOYEHT_IOS_SIGNING_IDENTITY="Apple Distribution: <Name> (<TEAM_ID>)"   # or the SHA-1
export SOYEHT_DEVELOPMENT_TEAM=<TEAM_ID>
nice -n 15 uv run python scripts/package_ios.py                  # archive, sign, export, check
uv run python scripts/package_ios.py --validate                  # and ask App Store Connect to validate
uv run python scripts/package_ios.py --verify-only <file.ipa>    # check a file already exported
```

It needs a distribution certificate in the keychain and an App Store profile for the identifier,
both made in the developer account and never kept here. The profile is named in
`apps/ios/ExportOptions.plist` (`Soyeht Next iOS App Store`); `SOYEHT_IOS_PROFILE` names another.
The team and the identity are added to a copy of that file from the environment; neither is written
in the repository. The result is `dist/ios/soyeht-<version>-ios-aarch64.ipa` with its `.sha256`, and
the app's debug symbols in `dist/ios/symbols/` (keep them with the release). The `.ipa` is not part of
the GitHub release (`release_flow.py` collects `dist/`): the app goes through the App Store.

The steps, which are the manual ones of the earlier releases:

1. `xcodebuild archive -configuration Release -destination generic/platform=iOS` with
   `SOYEHT_APP_BUNDLE_ID`, `SOYEHT_PRODUCT_NAME` and `CODE_SIGNING_ALLOWED=NO`. Signing the whole
   archive by hand fails: a manual profile specifier reaches the Swift package targets, which
   take none (“does not support provisioning profiles”).
2. Sign the archived app with the distribution identity and an entitlements file made from
   `apps/ios/SoyehtPhone.Release.entitlements` plus `application-identifier`,
   `com.apple.developer.team-identifier` and `get-task-allow` false. That file declares no
   capability, because push notifications are off in 0.1.0: the App ID needs no Push Notifications
   capability, and the check below refuses an `aps-environment` in the exported app. (The Debug
   build keeps `apps/ios/SoyehtPhone.entitlements`, which has the development push entitlement for
   [push-notifications.md](push-notifications.md); when push ships, the Release file gets it too, and
   the script sets it to `production`.) An unsigned archive carries no entitlements, so without this
   step the exported app has none of them.
3. `xcodebuild -exportArchive` with `method` `app-store-connect` and manual signing.
4. The check of the file: the identifier, version and build number of `contracts/release.json`, the
   entitlements, a signature by an Apple Distribution certificate of the team, and an App Store
   profile (no listed devices, no debugging).

`--validate` runs `xcrun altool --validate-app`, which only asks App Store Connect whether it would
accept the file. It needs the same App Store Connect API key the Mac notarization uses
(`SOYEHT_NOTARY_KEY_FILE`, `SOYEHT_NOTARY_KEY_ID`, `SOYEHT_NOTARY_ISSUER`) and stops at once when
they are not set. The script never uploads: upload the file by hand with `xcrun altool --upload-app`
(or Transporter) after you have read the check.

The app declares the answers the store asks for in the repository:

- `ITSAppUsesNonExemptEncryption` is `false` in `apps/ios/Info.plist`. The app encrypts no content
  of its own. The wire to the Mac is `ws://` over the tailnet, which Tailscale encrypts; the app
  uses the system's TLS only for the `https` downloads of the optional dictation models, and
  CryptoKit only to authenticate the device and the Mac: Ed25519 signatures, SHA-256 hashes and an
  HMAC in the pairing challenge. None of it protects the confidentiality of data.
- `apps/ios/Resources/PrivacyInfo.xcprivacy` says the app does not track and collects no data. It
  declares the required-reason APIs the built app imports: `UserDefaults` (`CA92.1`, the app's own
  settings), file timestamps (`C617.1`: SwiftTerm's inline images call `stat()` and `fstat()` on a
  path the terminal program sends, and mlx-swift's `fmt` calls `fstat()`; the app's sandbox limits
  the paths they can reach, and they read only the type and size, never a timestamp) and
  system boot time (`35F9.1`, `DispatchTime` uptime for the request timer). The app talks to the
  person's own Mac. When new code or a new dependency uses another API of Apple's list (file
  timestamps, system boot time, disk space, active keyboards, user defaults), check `nm -u` of an
  `iphoneos` build and add its category and reason there.

`scripts/check_ios.sh` fails when the built app has no manifest, when the manifest lacks the
file-timestamp category, or when Info.plist has no encryption answer.

The store record and the name under the icon of a store build are both “Soyeht Next”, as the
production Mac app is (the name “Soyeht” belongs to an older record). Text that tells the person
to open the app reads the display name (`AppName`), so the iPhone names the Mac app correctly and
no string names a build.

On the new Mac app, choose “Pair iPhone or iPad”. On the
device, choose “Scan Mac QR”. Compare fingerprints and allow access on the Mac.
Open a pane from the list.
A paired iPhone or iPad is a full remote control of the Mac (owner decision,
2026-09-26). Tap a terminal to type: the software keyboard has an Esc, Ctrl,
Tab and arrow row, and a hardware keyboard sends Return, Backspace, Tab, arrows,
Ctrl-C, Ctrl-D and Esc. For agent panes it also sends prompts, answers
permission requests and changes the harness's permission mode after engine confirmation. Both devices must be connected to the same tailnet.

## Layout

At compact width (iPhone, or an iPad app in a narrow Split View or Stage
Manager window) the home screen is the list of workspaces and their panes, and
choosing a pane pushes it full screen. At regular width the same list becomes a
sidebar and the detail column shows the selected pane's workspace as a board:
its panes side by side, arranged from the layout the Mac stores for the workspace
(`workspace.layout.set`), with the same fit rules as the Mac and the desktop
(`PaneLayoutFit`, ADR 0099). Panes that do not fit side by side stack, and when
they do not stack either they share one tab group. The board never edits the
stored layout; a wider window shows the stored arrangement again. A tap on a pane
focuses it (the accent outline), and the focused pane takes the keyboard. Each
pane is an agent conversation, a terminal with input, or a web pane's “Open on the
Mac”; an editor pane is shown on the Mac only. A workspace with one pane, or with a
writing pane (the writing workspace arranges that pane and its companion itself),
shows the selected pane alone, as before. The detail starts on the pane focused on
the Mac. When the focused pane closes, focus moves to the nearest pane in the same
workspace, then anywhere; at compact width the app returns to the list instead. The
app supports every iPad orientation, Split View and Stage Manager.

## Panes and workspaces

The “+” of the sidebar opens a new pane or a new workspace; each workspace header
has a menu to open a pane in it, rename it or close it.

- A new pane is a terminal, an agent (harness, model, effort, permission mode and an
  optional first message) or a writing document, started in a folder on the Mac. The
  sheet edits the same `NewPaneForm` as the Mac's and follows the same rules.
- A new workspace is made for a folder. The Mac lists folders only inside workspaces it
  already has, so the picker offers the folders the Mac works in, their sub-folders, and a
  path field; the Mac refuses a path that is not an existing folder. A folder that already
  has a workspace opens that one.
- A pane closes from a swipe, the context menu of its row, or the button above the pane.
  Terminals, agents and web panes ask first (“The process in this pane will be
  terminated.”); a document asks only while it has unsaved changes. Closing a workspace
  asks when it holds panes, since they close with it; its folder is not touched. The Mac
  asks the same when the close button of a workspace tab is pressed; a close sent to the
  engine by a script or an agent never waits on a dialog. What was typed in the workspace's
  documents is sent before it closes. `PaneClosePolicy` holds these rules for both apps.

## Hardware keyboard

A terminal takes the keyboard as soon as it is shown at regular width or while
a hardware keyboard is attached, so typing needs no tap first. The shortcuts
also appear in the iPad menu bar and when holding Command:

| Shortcut | Action |
| --- | --- |
| Command-] / Command-[ | Next or previous pane: across workspaces in the list, in visual order on a board |
| Option-Command-arrow | On a board, focus the pane beside the focused one |
| Control-Command-S | Show and focus the sidebar |
| Command-1 … Command-9 | Jump to that workspace |
| Command-Return | Send the agent prompt (Return adds a line) |
| Shift-Tab (agent prompt) | Move to the harness's next permission mode, as on the Mac |
| Command-= / Command-- / Command-0 (terminal) | Larger, smaller and default terminal text |
| Command-C / Command-V (terminal) | Copy the selection, paste the clipboard |

Arrows, Page Up and Down, Home, End, Esc, Tab, Shift-Tab, function keys and Control
combinations go to the terminal through SwiftTerm. Two-finger scrolling on a trackpad
moves the scrollback; a full-screen program that asks for the mouse gets wheel events, and
one on the alternate screen gets arrow keys.

## Apple Pencil

Handwriting (Scribble) works wherever the person types. The agent prompt, the text fields of
the sheets and the writing editor are system text inputs, which take Scribble themselves. The
terminal is a custom text input, so `HandwritingArea` (`apps/shared`) makes its whole area a
writable element: writing on a terminal gives it the keyboard focus (and, on a board, the
pane's focus) and the recognised text goes to the terminal like typed text. A Pencil also taps,
scrolls and focuses like a finger. The simulator cannot write with a Pencil, so Scribble is
checked on an iPad with one.

## Terminal text and size

The terminal menu (top right of a terminal pane) and a pinch change the text size, one
setting for every terminal between the limits of `paneLayout` in
`contracts/design-tokens.json`.

By default the iPad uses the size the pane already has and never reflows the Mac; a pane wider
than the screen scrolls sideways. “Fit to this screen” in the same menu sizes the pane to the
iPad instead, and “Use the Mac's size” asks for the size the pane had before the fit. The
choice lasts until the app closes (ADR 0013). When the iPad is the only viewer, for example
a pane the Mac made and no longer shows, the pane keeps the last size it had, which can look
small; “Fit to this screen” fixes that.

Links open in Safari, or in a web pane on the Mac for an address on the Mac itself. Settings
has a Terminal section with the web-link switch and “Let programs copy to the clipboard”, off
by default: when on, a program can copy to the iPad's clipboard while the app is open and the
terminal is on screen, and the app says “Copied by the terminal”. A program can never read the
clipboard.

## Access

On returning from the background, wait for full synchronization. If access was
revoked, pair again. “Forget this Mac” removes the device's credential; revoking
on the Mac ends access at the engine and closes existing connections.

Without a compatible camera, the same link can be pasted or opened in the app.
The QR expires in ten minutes and only one approval consumes the code.
Never put the link, token or private key in a report or issue.
