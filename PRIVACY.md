# Privacy policy

Last updated: 2026-11-08

This policy covers Soyeht Next: the Mac app, the Windows and Linux app, the iPhone and iPad app,
the Android app (not yet released) and the engine and command line that run on your computer. Soyeht is
open source (Apache-2.0) and is published by the maintainers of
[github.com/soyeht/soyeht](https://github.com/soyeht/soyeht). Every statement below can be checked
against the source in that repository.

## In short

- **We collect nothing.** Soyeht has no account, no sign-in and no server of ours that your apps
  talk to. We receive no data about you, your devices, your terminals or your agents.
- **No analytics, no advertising, no tracking.** None of the apps contains an analytics,
  advertising or crash-reporting library, and the iPhone and iPad app declares in its privacy
  manifest that it does not track you and collects no data. The one exception is Google's ML Kit
  in the Android app, only if you turn on handwriting on the device: it sends diagnostics and usage metrics to
  Google (see "Handwriting on Android" below). What you write is not among the data Google lists.
- **Your work stays on your computer.** Terminals, prompts, agent conversations, files and settings
  live on the computer that runs the engine. Your iPhone, iPad or Android phone talks only to that
  computer, over your own private network.
- **A few requests leave your devices, and each one is listed in this document**: the update check,
  dictation, optional model downloads (and, on Android, the diagnostics and usage metrics of
  Google's handwriting library), the usage meters, and the agents and web pages you choose to run.
  None of them goes to a server run by Soyeht. Push notifications are not part of this release
  (see "Push notifications" below).

## What stays on your devices

**On the computer that runs the engine** (the Mac, or the Windows or Linux computer), Soyeht keeps
in its own data folder: your workspaces and panes, conversations and tasks, settings, the list of
paired devices and their public keys, and log files. The
folder and its files are readable only by your user. Nothing in it is uploaded. The logs record
event names and failure kinds, never the text of a message, the output of a terminal or a file path
(`docs/adr/0088-one-platform-log-and-failure-streaks.md`).

**On your iPhone or iPad**, the pairing credential (a private key made on the device, the access
token the Mac issued, the Mac's public key and its address) is stored in the iOS Keychain, readable
only while the device is unlocked, and it does not move to another device through a backup or
iCloud Keychain (`apps/shared/Sources/SoyehtClient/CredentialVault.swift`). Appearance settings
such as the theme and the terminal text size are kept in the app's own preferences. The camera
reads the pairing QR code on the device; the image is not stored or sent.

**On Android** (the app is not published yet), the credential is sealed with a key in the Android
Keystore, in the app's private storage, and the app opts out of backup. It asks for two
permissions: network access, which it uses to reach your computer, and the camera, which reads the
pairing QR code; the image is not stored or sent.

You can remove all of it. "Forget this Mac" in the phone app deletes the credential from the phone
and asks the Mac to drop the phone's push token when the Mac is reachable; revoking the phone on
the Mac ends its access and deletes its push token there. `soyeht uninstall --delete-data` removes the data
folder on the computer (`docs/adr/0118-uninstall-keeps-data-and-a-redacted-diagnostics-report.md`).

## How your devices talk to each other

The phone app connects to the engine on your computer over a WebSocket. The app accepts only a
loopback or Tailscale address (`100.64.0.0/10` or the tailnet's IPv6 range) and refuses any other
host, and the engine listens only on loopback and tailnet addresses, never on your local network or
the internet (`docs/adr/0078-tailnet-listeners-follow-interfaces.md`). The connection is encrypted
by your tailnet. Pairing is a one-time QR code whose fingerprint you approve on the computer; the
phone checks the engine's signature on every connection and the engine checks the phone's
(`docs/adr/0132-the-engine-proves-itself-to-a-paired-device.md`). There is no Soyeht relay between
them.

## What leaves your devices

### Update check (Mac, Windows and Linux)

Once at start and then once a day while the app runs, the app asks GitHub for a small signed file,
`update-manifest.json`, at
`github.com/soyeht/soyeht/releases/latest/download/update-manifest.json`. The request is a plain
HTTPS `GET`. It carries the User-Agent `Soyeht-update-check` and the headers the system's HTTP
stack adds, and it carries no version number, system, installation identifier, cookie or account.
GitHub, like any server, sees the IP address of the connection. The app compares the answer on your
device against a signature key built into the app, and only tells you that a newer version exists;
it installs nothing. You can turn the check off in Preferences, and then nothing is fetched
(`docs/contracts/15-updates.md`, sections 5 and 6; `contracts/update.json`). The iPhone and iPad app
has no update check: the App Store updates it. If you download Soyeht from GitHub, GitHub's own
privacy statement applies to that download.

### Push notifications (iPhone and iPad)

**Push notifications are not part of Soyeht 0.1.0.** The iPhone and iPad app does not ask for
notification permission, does not ask Apple for a push token and does not send one to your
computer, and the engine on your computer sends nothing to Apple's push service (APNs). The app
does these things only when the engine it is paired with says it can send push alerts, and an
engine says so only when it has been given Apple push credentials (`docs/push-notifications.md`),
which only works for an app build you sign yourself. The release engine has none. An engine
without them also refuses a token if a program sends one (`push_unavailable`) and keeps nothing.

Push is meant to return through a relay. Until then, if you turn it on for your own build, this is
what happens: when an agent stops to ask you something and a phone is paired, the engine sends an
alert straight to APNs, with no Soyeht server in between. The message contains the same generic
title and text for every question ("An agent needs you. Open Soyeht to see what it asks."), plus
random identifiers of the Mac, workspace, pane and request so that a tap opens the right pane. It
never contains what the agent asked: no command, file path or question text. The phone shows that
detail itself, from its own private connection to your computer
(`engine/soyeht-engine/src/push/payload.rs`, `contracts/vectors/push-permission.json`). To deliver
the alert, your phone's push token is registered on your computer and used from there with Apple.
Apple handles the token and the alert under its own privacy policy.

### Dictation (voice to text in the message box)

Dictation is off until you tap the microphone. The app then asks for microphone and speech
recognition permission. By default it uses Apple's speech recognition and asks for on-device
recognition whenever the device supports it for your language
(`apps/shared/Sources/SoyehtClient/AppleSpeechRecognizer.swift`). For a language the device cannot
recognise on the device, the system's speech service may process the audio on Apple's servers, as
Apple documents; Soyeht does not receive that audio. The optional NVIDIA Nemotron engine always
runs on the device. The audio never goes to us.

### Model downloads (optional)

The Nemotron dictation model (about 642 MB) is downloaded only after you press Install in
Preferences. It is fetched from the Hugging Face model hub (`huggingface.co`) and then runs
entirely on your device; nothing about you is sent beyond what any download request reveals, such as
the IP address. You can delete it again with "Remove download" in Preferences. Nothing is downloaded before you ask.

### Handwriting on Android (optional, off by default)

On Android 13 and older you can turn on "Handwriting on this device" in Settings: the pen then writes in the message
field, read on the device by Google's ML Kit Digital Ink Recognition. It is off until you turn it on. While it is off,
the app does not start ML Kit (the component that would start it at launch is removed from the app) and no new data is
collected. Metrics that ML Kit queued before you turned it off may still be sent later, because its transport can run
again when the app starts. Turning it on starts ML Kit, and from then on **ML Kit sends diagnostics and usage metrics
to Google**. The
[ML Kit data disclosure](https://developers.google.com/ml-kit/android-data-disclosure) lists them as: the device's
manufacturer, model, system version and build, and the machine-learning hardware accelerators it has; the app's package
name and version; the API configuration; the size of the input and of the output; the feature version; performance
metrics such as latency; events (feature initializations, model downloads, recognitions, resource releases) and their
error codes; the languages configured; and per-installation identifiers, which Google says are not intended to uniquely
identify a user or a physical device. Google says it encrypts this data in transit (HTTPS) and does not transfer it to
third parties. The [ML Kit terms](https://developers.google.com/ml-kit/terms) require us to tell you this.

The content of what you write (your strokes, and the text read from them) is not in the data Google lists, and the
ML Kit terms say that the processing of the input "fully happens on-device" and that ML Kit "does not send that data
and the resultant outputs to Google servers"; Soyeht sends it nowhere either. After you turn it on, the language model
is not downloaded until you press Install, which fetches it from Google's servers; like the download above, that shows
your device's IP address to Google. Outside Install, once it is on, the terms also say ML Kit "may contact Google
servers from time to time in order to receive things like bug fixes, updated models", which shows the address too. You can remove the model there. Soyeht itself sends nothing. While it is off the pen
works as a finger. On Android 14 and later the system's own handwriting is used and ML Kit is not involved. If the app
is listed on Google Play, its data-safety form declares these data
(`docs/adr/0157-the-android-app-writes-with-the-pen-on-the-message-field.md`).

### The agents you run

Soyeht runs the coding agents you choose (Claude Code, Codex, Gemini CLI, OpenCode and others) in
panes on your computer. Each agent talks to its own provider, under your own account and your own
sign-in, with your own terms with that provider. Soyeht does not proxy those
conversations or send them anywhere: the engine keeps the transcript in its data folder on your
computer, and the provider's privacy policy governs what the agent sends to the provider. When a pane starts a
built-in agent, your package runner (`npx` or `uvx`) may download the pinned adapter package for it
from the public package registry the first time
(`engine/soyeht-engine/src/agents/acp_registry_snapshot.json`). A pane can also run any program you
can run in a terminal, with the network access that program has.

### Usage meters

The usage rings show how much of an agent subscription you have used. The engine reads the login
that the agent itself keeps on your computer (for example Claude Code's, Codex's or Cursor's) and
asks that provider's own usage endpoint, read-only, from your computer: `api.anthropic.com` or
`claude.ai`, `chatgpt.com` or `auth.openai.com`, `cursor.com`, and for OpenCode `opencode.ai` or the
Z.ai quota API (`api.z.ai`, `open.bigmodel.cn`) when OpenCode is set to use them. A credential is sent only to the provider that issued it, is never
stored by Soyeht, never appears in a log, and is never sent to us
(`docs/contracts/10-app-bar-and-usage.md`, section 3).

### Web panes

A web pane shows a page you open, in the system's web view on your computer, with the network access
any browser has. The agents you run can read that page through the engine on your computer; what they
do with it is governed by the agent's provider as above.

### Crash reports and diagnostics

When a Soyeht program crashes on your computer, it writes a small report to its data folder: the
version, the system, the kind of failure, the source location and the call-stack offsets, and the
last structured log lines. It never records the panic message. The next time the app opens it tells
you the report was saved and that nothing was sent. **Nothing is uploaded.** "Copy report" and "Copy
diagnostics" put a redacted text on your clipboard for you to paste into an issue if you choose; the
redaction removes home folders, paths, links, e-mail and network addresses, host and user names and
secrets (`docs/contracts/16-crash-reports.md`, `docs/contracts/14-diagnostics.md`,
`docs/adr/0127-crash-reports-are-local-and-sent-on-purpose.md`). The iPhone and iPad app has no
crash reporter of its own. If you turned on "Share with App Developers" in iOS settings, Apple may
share crash logs of the app with the maintainers through App Store Connect; that is Apple's
service under Apple's privacy policy.

### What you post yourself

If you open an issue or a pull request on GitHub, what you write is public and governed by GitHub's
privacy statement. Please do not paste secrets or other people's data.

## What we do not do

We do not sell or share personal data, because we do not have any. We do not build profiles, show
advertising, use third-party analytics or tracking SDKs, or combine data across apps. The iPhone and
iPad app does not use App Tracking Transparency because it does not track. Apple may give the
maintainers aggregated, non-identifying App Store statistics, such as download counts. That is
Apple's service.

## App Store privacy answers

For the App Store listing of Soyeht Next for iPhone and iPad, the data-collection answer is **Data
Not Collected**. The app sends nothing to the developer or to any third party that the developer
controls, and the listing's privacy manifest is `apps/ios/Resources/PrivacyInfo.xcprivacy`. The
sections above describe the system services that act on your behalf: Apple's speech services,
and downloads you start yourself.

## Children

Soyeht is a developer tool and is not directed to children. Because it collects no data, it holds
no data about children either.

## Changes to this policy

A change to this policy is a change to this file in the public repository, with its date at the top
and its history in the repository. A change that adds anything that leaves your devices is made in
the same pull request as the code that does it.

## Contact and support

Questions, corrections and support requests go to the issue tracker:
[github.com/soyeht/soyeht/issues](https://github.com/soyeht/soyeht/issues). Please report a
security problem privately, as [SECURITY.md](SECURITY.md) explains, and not in a public issue.
