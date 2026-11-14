# App Store submission: iPhone and iPad

This document holds everything the App Store Connect record of the iPhone and iPad app needs, so
that the owner's part is to paste text and click. It was written against the code at the commit
that added it; every claim about what the app does cites the file and line it comes from. Apple's
limits and screenshot sizes change, so the sizes and form wording below are the ones this document
knew on 2026-11-14: App Store Connect shows the current ones next to each field, and where it
disagrees, it wins. The build itself is made by `scripts/package_ios.py`
([docs/apps-iphone.md](apps-iphone.md), "Store build"; [docs/release.md](release.md)); nothing here
uploads anything.

## 0. The record in one table

| Field | Value | Where it comes from |
| --- | --- | --- |
| Bundle ID | `com.soyeht.next.ios` | `APP_ID` in `scripts/package_ios.py` and the profile of `apps/ios/ExportOptions.plist`. It is not in `contracts/profiles.json`, and the Xcode project builds the Dev identifier `com.soyeht.next.dev.ios` unless the packager overrides it (ADR 0112). |
| Name under the icon and name of the record | Soyeht Next | `docs/apps-iphone.md`, "Store build"; the production Mac app is also "Soyeht Next". |
| Version | 0.1.0 | `contracts/release.json`; `MARKETING_VERSION` of the project. |
| Build number | the one `package_ios.py` writes from `contracts/release.json` | The Xcode project carries a development number; the packager sets the real one. A second upload of one version needs a new build number (`release.py prepare revision`). |
| Devices | iPhone and iPad, one binary | `TARGETED_DEVICE_FAMILY = 1,2`, so iPad screenshots are required. |
| Minimum system | iOS 18.0 | `IPHONEOS_DEPLOYMENT_TARGET` in `apps/ios/SoyehtPhone.xcodeproj/project.pbxproj`. |
| Price | Free, no in-app purchases | The code imports no StoreKit; there is nothing to sell. |
| Account or sign-in | None | There is no account in the app or the engine (PRIVACY.md, "In short"). |
| Push notifications | Off in 0.1.0 | `PhonePush.connected` asks for nothing unless the engine offers push, and the release engine never does (`apps/ios/Sources/PhoneNotifications.swift:86-88`, `docs/push-notifications.md`). |

## 1. Listing

Apple's limits are in characters except the keywords, which count bytes. The counts below were
measured, not estimated (all text here is plain ASCII, so characters and bytes are equal).

| Field | Limit | Used |
| --- | --- | --- |
| Name | 30 | 11 |
| Subtitle | 30 | 26 |
| Promotional text | 170 | 156 |
| Description | 4000 | 2478 |
| Keywords | 100 bytes | 97 |
| What's new | 4000 | 14 |
| App Review notes | 4000 | 3232 |

### Name

```text
Soyeht Next
```

If App Store Connect says the name is taken (the name "Soyeht" belongs to an older record, and
names are unique in the store), use "Soyeht Next for iPhone" or "Soyeht Next Companion". Keep the
Mac app's display name and this one equal: the Mac's pairing sheet tells the person to open the
app by its name (`apps/shared/Sources/SoyehtClient/AppName.swift`, `PairingCopy.swift:16`).

### Subtitle

```text
Your Mac's agents, in hand
```

### Promotional text

Editable at any time without a new review.

```text
Pick up your coding agents and terminals from your iPhone or iPad. Pair with your Mac over your private network, then type, answer and review from anywhere.
```

### Description

```text
Soyeht Next is the iPhone and iPad companion of Soyeht for Mac. It needs Soyeht running on your own Mac (a free download from github.com/soyeht/soyeht) and Tailscale on both devices, signed in to the same tailnet. Without them the app only shows how to pair.

Soyeht runs your coding agents as a team, each in its own pane on your Mac. This app puts the same workspaces, terminals and agents in your hand, so a conversation you started at your desk can keep going when you step away.

WHAT YOU CAN DO

- Pair once. Scan the QR code the Mac shows, compare the short code on both screens and allow access on the Mac. The pairing can be revoked from the Mac at any time, or removed from the phone with "Forget this Mac".
- Open any pane of your Mac: an agent conversation, a terminal, or a writing document.
- Drive agents: send prompts, answer permission requests, switch the agent's permission mode, model and effort.
- Type in real terminals. The software keyboard has Esc, Ctrl, Tab and arrow keys; a hardware keyboard works like the one on your Mac, with shortcuts in the iPad menu bar.
- Start new panes and workspaces in the folders your Mac works in, and close the ones you no longer need.
- Watch your usage: rings show how much of each agent subscription you have used.
- Dictate messages with the microphone in the composer. Dictation uses Apple's speech recognition, on the device when your language supports it. An optional on-device model can be installed from Settings.
- Write with Apple Pencil in any text field, including the terminal.
- Use the whole iPad: every orientation, Split View, Stage Manager, and a board that shows a workspace's panes side by side.

PRIVATE BY DESIGN

Soyeht has no account, no server of its own, no analytics and no advertising. Your terminals, prompts and files stay on your Mac. The app talks only to your own Mac, over your private network, and refuses any other address. The pairing key is stored in the iOS Keychain on this device only.

GOOD TO KNOW

- A paired iPhone or iPad can do what you can do at the Mac, including typing in your terminals. Pair only devices you own, and compare the codes every time.
- The agents run on your Mac, under your own accounts with their providers. Soyeht does not provide them or their models.
- Pairing works with Soyeht for Mac.
- Push notifications are not part of this version.
- Soyeht is open source (Apache-2.0). Source, issues and the privacy policy are at github.com/soyeht/soyeht.
```

The first paragraph says what the app needs before it says what it does. That is deliberate: an
app that does nothing without another product must say so on its page (Guideline 2.1, and
Guideline 2.3 on metadata that matches the app). Two choices to know about:

- The description names Tailscale once, as a requirement, because the person cannot pair
  without it. It is a third party's name, used only to say what the app works with. Take it out
  if you would rather not rely on that, and keep "your private network".
- It does not name Claude Code, Codex, Cursor or OpenCode. Their names are other companies'
  marks, and metadata that uses them invites a Guideline 2.3.7 or 5.2 question. The app does
  show the providers' logos in its composer (finding F8), but the listing does not need to.

### Keywords

Comma-separated, no space after a comma, no repeat of the name or the category (Apple already
indexes those), and no other company's mark (so no "tailscale", "claude" or "ssh": the app does
not use SSH, and a keyword that does not describe the app is a rejection reason).

```text
terminal,developer,coding,agents,remote,mac,companion,shell,console,workspace,pencil,dictation,ai
```

### What's new in 0.1.0

The first version of an app has no "What's New" field in App Store Connect; it appears from the
second version. Keep this text for then, and for the TestFlight "What to Test" field.

```text
First release.
```

### URLs and other fields

| Field | Value |
| --- | --- |
| Support URL | `https://github.com/soyeht/soyeht/issues` (the place PRIVACY.md and README.md give for help) |
| Marketing URL | `https://github.com/soyeht/soyeht` (no other website exists; do not invent one) |
| Privacy Policy URL (required) | `https://github.com/soyeht/soyeht/blob/main/PRIVACY.md` |
| Privacy Choices URL | leave empty; there is nothing to opt out of |
| Copyright | `2026 <the legal name of the publisher>`; only the owner knows it |
| SKU | any private string, for example `soyeht-next-ios` |
| Primary language | English (U.S.) |
| Version release | Manually release (see the checklist: the Mac download must be public first) |
| Price and availability | Free, all territories |

The privacy policy is a Markdown file that GitHub renders; it is public, dated and versioned in the
repository, which is acceptable as a URL. If the repository is ever renamed or made private, the
URL must change in the same pull request that does it.

### Categories

- Primary: **Developer Tools**. The app is a companion for people who run coding agents and
  terminals.
- Secondary: **Productivity**.

### Age rating

Apple's questionnaire was rebuilt in 2025 (ratings of 4+, 9+, 13+, 16+ and 18+, with new
questions). The answers below are by topic, so they hold whichever way the form words them. Expected
result: **4+**. Answer each form question by the topic it asks about; if the form has a question
this table does not cover, answer it truthfully and keep the reason as a note here.

| Topic | Answer | Reason |
| --- | --- | --- |
| Violence (cartoon, realistic, graphic) | None | The app makes no content of its own. |
| Sexual content or nudity | None | Same. |
| Profanity or crude humour | None | Same. |
| Horror or fear themes | None | Same. |
| Alcohol, tobacco or drug references | None | Same. |
| Medical or wellness information | None | Not a health or wellness app. |
| Gambling, contests, loot boxes | None | Nothing is sold or won (no StoreKit). |
| Unrestricted web access | No | There is no browser in the app. A link in a terminal opens in Safari or in a web pane on the Mac (`apps/shared/Sources/SoyehtClient/PhoneTerminalLinks.swift:12-24`). Only HTTP and HTTPS links open at all. |
| User-generated content | No | Nothing the person writes is shared with other people. |
| Messaging and chat between users | No | The person talks to agents on their own computer; the app has no way to reach another person. |
| Advertising | No | No advertising or analytics code. |
| Parental controls, age assurance | No | None are built. |

One thing to say if the form asks about generated text: the agents the person runs on their own
Mac write text (code, prose) under the person's own accounts with the providers; Soyeht does not
provide, host or filter them (README.md, "Status"). If Apple raises the rating because of that,
accept the higher rating rather than argue it; a 13+ or 16+ would not block the release.

## 2. App Privacy ("nutrition label")

**Answer: "Data Not Collected"**, and the app does **not** track. This is what PRIVACY.md says
("App Store privacy answers") and what `apps/ios/Resources/PrivacyInfo.xcprivacy` declares, and
the code supports it. Apple's definition of "collect" is data sent off the device in a way that lets
the developer or a third-party partner read it for longer than needed to serve the request. The
developer runs no server and receives nothing, so no category applies.

### Where the data goes, from the code

| Data the app handles | Where it goes | Why it is not "collected" | Evidence |
| --- | --- | --- | --- |
| Terminal input, prompts, answers to permission requests, attached photos and files | The person's own Mac, over a WebSocket on the tailnet or loopback | The developer has no access; the Mac is the person's own. | `Wire.swift:78-80` opens the socket; `DeviceIdentity.swift:41,96-107` accepts only loopback and `100.64.0.0/10` or the tailnet IPv6 range; attachments in `AgentComposerAttachmentViews.swift:97-125` |
| Pairing credential: a private key made on the device, the access token, the Mac's public key and address | The iOS Keychain, `WhenUnlockedThisDeviceOnly`, so not in a backup or iCloud Keychain | Stays on the device. | `CredentialVault.swift:27`, `DeviceIdentity.swift:219` |
| The device's name | The person's Mac, once, in the pairing request, so the Mac can show "Allow access for ...". | Goes to the person's own computer, not to the developer. | `PhoneModel.swift:66` |
| Camera image | Read on the device for a QR code, then dropped | Never stored or sent. | `QRScanner.swift:41`, `PhoneHome.swift:193` |
| Voice for dictation | Apple's speech recognition, on the device when the language supports it | Apple's own framework; the developer never receives the audio. If the language cannot be recognised on the device, Apple's service may process it under Apple's terms. | `AppleSpeechRecognizer.swift:36` (`requiresOnDeviceRecognition` is set whenever the recognizer supports it), `MicrophoneCapture.swift:17-45` |
| Dictation model (optional, about 642 MB) | Downloaded from the model host after the person presses Install, then runs on the device | Nothing about the person is sent beyond what any download shows (the IP address, which is not one of Apple's data types unless it is used for a location or an identifier, and it is not). | `NemotronModel.swift:41-52`, PRIVACY.md "Model downloads" |
| Settings (theme, text size, clipboard switch, dictation vocabulary) | `UserDefaults` on the device | Never leaves it. | `PrivacyInfo.xcprivacy` reason `CA92.1`; `DictationVocabulary.swift:39` |
| Clipboard | A program in a terminal may copy to the clipboard only if the person turned that on, and the app can never read it | Off by default; write-only. | `SystemClipboard.swift:17`, `TerminalClipboardPreferences.swift:13-18` |
| Push token | Not requested in 0.1.0 | The release engine never offers push, so the app never asks Apple for a token. | `PhoneNotifications.swift:86-100` |
| Crash logs | None from the app itself; Apple may pass crash logs of people who chose "Share with App Developers" through App Store Connect | That is Apple's service. | PRIVACY.md "Crash reports and diagnostics" |

### Data types to tick: none

Contact info, health and fitness, financial info, location, sensitive info, contacts, user
content, browsing history, search history, identifiers, purchases, usage data, diagnostics and
other data: **not collected** in every category. Tracking: **No**; there is no advertising
identifier, no `AppTrackingTransparency`, no `AdSupport` and no tracking domain
(`NSPrivacyTracking` is false and `NSPrivacyTrackingDomains` is empty in
`PrivacyInfo.xcprivacy:8-11`). The app has no `NSUserTrackingUsageDescription`, and must not get one.

### PRIVACY.md and the privacy manifest agree

| Statement | PRIVACY.md | `PrivacyInfo.xcprivacy` | Code |
| --- | --- | --- | --- |
| No tracking | "declares ... that it does not track you" | `NSPrivacyTracking` false, no domains | no tracking frameworks |
| No data collected | "Data Not Collected" | `NSPrivacyCollectedDataTypes` empty | see the table above |
| Required-reason APIs | not listed (not a privacy claim) | `UserDefaults` CA92.1, file timestamp C617.1, boot time 35F9.1 | checked by the authors against `nm -u` (the file's own comment) |
| Update check | "The iPhone and iPad app has no update check" | no network API declared for it | `UpdateChecker` exists in `apps/shared` but nothing in `apps/ios` or the shared views starts it; it is a Mac and desktop feature |

No contradiction was found. Two small gaps in PRIVACY.md are listed as findings F7 and F8 below.

The manifest covers the app's own code and the Swift packages it builds from source (SwiftTerm,
the dictation package and what it pulls in). `scripts/package_ios.py --validate` asks App Store
Connect whether it would accept the file; after the real upload Apple emails about any
required-reason API the build uses that the manifest lacks (ITMS-91053). Treat such an email as a
manifest edit, not a policy question.

## 3. Export compliance

`ITSAppUsesNonExemptEncryption` is `false` in `apps/ios/Info.plist:26-28`. With that key present,
App Store Connect does not ask the export-compliance questions again for each build.

The basis, stated conservatively:

1. **The wire to the Mac is not encrypted by the app.** It is a plain `ws://` WebSocket
   (`Wire.swift:78-80`), and the network protects it: Tailscale encrypts the tailnet (WireGuard),
   which is another app's feature, not this one's. Contract 06, section 1, records this.
2. **The only TLS the app can start is the operating system's.** The optional dictation model
   download is HTTPS through the system's networking, and the Keychain and Apple's speech
   service are the system's too. Encryption done by the operating system's own APIs, with no
   algorithm of the app's own, is the case Apple's flow calls exempt.
3. **The app's own cryptography is authentication only.** It uses CryptoKit for Ed25519
   signatures (`DeviceIdentity.swift:151,191,219`), SHA-256 fingerprints (`:222`) and an HMAC for
   the pairing challenge (`:229`). It encrypts no user data with them. Signing, verifying and
   hashing for authentication, and not for the confidentiality of data, is outside the controlled
   encryption of Category 5 Part 2 (the exclusion for authentication and digital signature).
   There is no AES, ChaCha or other cipher call in `apps/ios`, `apps/shared` or `apps/dictation`
   (a grep for them found none).

If App Store Connect asks anyway (it does when the key is missing from a build):

| Question | Answer |
| --- | --- |
| Does your app use encryption? | Yes (HTTPS and the Keychain count, even through the system) |
| Is your app exempt, or does it qualify for an exemption? | Yes: the encryption is limited to the operating system's own, and the app's own cryptography is for authentication and digital signatures only |
| Does it use proprietary or non-standard algorithms? | No |
| Is it available in France? | Yes. An exempt app needs no declaration |
| Upload a document or self-classification | Not needed |

This is a statement of the facts in the code and how the exemptions read; it is not legal advice.
It should be re-read when the app changes: a cipher of the app's own (for example the app
encrypting its connection itself instead of the system doing it) or a dependency that does one
would change the answer. Before the upload, run `nm -u` on the built app and look for `CCCrypt`, `SecKeyEncrypt`
or a cipher symbol; the packager does not do this check.

## 4. App Review notes

### The companion problem

Without a paired Mac, the app shows only its pairing screen (`PhoneHome.swift:137-170`). It has no
sample data and no demo mode. A reviewer cannot pair on their own: the app accepts only a loopback or
Tailscale address (`DeviceIdentity.swift:96-107`), so a Mac on the reviewer's desk, a hosted engine on the
internet or a LAN address will not work, and a reviewer who has no Tailscale sign-in cannot reach
a tailnet at all. This is the biggest risk to the review (Guideline 2.1, "App Completeness": a
reviewer who cannot reach the features may reject; and 4.2, "Minimum Functionality").

What to provide, in this order of effect:

1. **A demo video (required).** Screen recordings of the real flow on an iPhone and an iPad: the
   Mac's "Pair iPhone or iPad" sheet, "Scan Mac QR", the two codes compared, "Allow" on the Mac,
   then a terminal with typing (software keyboard row of Esc, Ctrl, Tab and arrows), an agent
   pane with a permission request answered from the phone, dictation, a new pane from the "+",
   Settings with the usage rings, and the iPad board in landscape. Two to four minutes, no
   voiceover needed, captions on. Host it unlisted on a link that needs no login and attach the
   same file to App Review Information (App Store Connect takes common video files as attachments; the field names the allowed types). The
   notes below have a `<DEMO_VIDEO_URL>` to fill.
2. **A real Mac, offered as an option, not a dependency.** The notes give the reviewer the steps and
   the public download. Few reviewers will do it; those who do will see it work.
3. **A demo mode in the app (recommended, a product change).** A "See a sample workspace" button
   on the pairing screen that opens a read-only, offline workspace with canned panes would let
   the reviewer touch the interface without a Mac, and it would also make screenshots possible
   without a tailnet. It is not in this pull request: the app change belongs to the owner's
   decision and its own review. If the video is judged not enough, this is the fix that does not
   depend on the reviewer's effort.
4. **Not recommended: a hosted engine for the reviewer.** The app refuses a non-tailnet address,
   and putting an engine on the open internet to satisfy the review would contradict the product's
   design: the engine listens only on loopback and tailnet addresses ([PRIVACY.md](../PRIVACY.md),
   "How your devices talk to each other"). Tailscale signs people in through an identity provider,
   so giving the reviewer access would mean sharing a login of the owner's, and an auth key must
   never be put in review notes. Do not do either.

Before submitting for review, send the build to **external TestFlight** first. Beta App Review
applies the same guidelines with a lighter touch, and it answers the companion question (and any
other rejection) before the real submission does.

### Text for the "Notes" field

The notes field holds 4000 characters; this text uses 3232. Fill the two placeholders. Sign-in
required: **off** (there is no account, so no demo account either).

```text
Soyeht Next is the iPhone and iPad companion of Soyeht, a Mac app that runs the person's coding agents in terminal panes. The app has no account, no sign-in and no content of its own. It shows and controls the workspaces, terminals and agent conversations of the person's own Mac, over the person's own private Tailscale network. Without a paired Mac it shows only its pairing screen, so please use the demo video.

DEMO VIDEO (no sign-in needed): <DEMO_VIDEO_URL>
It shows, in about <N> minutes: installing Soyeht on the Mac, "Pair iPhone or iPad" on the Mac, "Scan Mac QR" on the phone, the comparison of the two four-group codes, "Allow" on the Mac, then a terminal, an agent conversation with a permission request, dictation, and the iPad board.

TO TRY IT YOURSELF you need an Apple silicon Mac with macOS 15 or later and Tailscale on both devices (free account), signed in to the same tailnet:
1. Download and open the Mac app from https://github.com/soyeht/soyeht/releases/latest (signed and notarized).
2. On the Mac choose iPhone in the sidebar, then "Pair iPhone or iPad". A QR code appears (valid for 10 minutes).
3. On the phone choose "Scan Mac QR" and allow the camera. If the camera is not available, open "Use a pairing link" and paste the link.
4. Compare the code on the phone with the code on the Mac. Choose "Allow" on the Mac only if they match.
5. Open a pane from the list. Tap a terminal to type.
The Mac app and its source are public: https://github.com/soyeht/soyeht

WHY THESE PERMISSIONS
- Camera: only to read the pairing QR code. The picture is not stored or sent.
- Microphone and Speech Recognition: only for the microphone in the message composer. Dictation uses Apple's speech recognition, on the device when the language supports it. Off until the person taps the microphone.
- Local Network: the Mac is reached by an address on the person's Tailscale network (or loopback). The app refuses any other address.
- No notifications: push notifications are not part of this version, and the app does not ask for notification permission.

NETWORK AND APP TRANSPORT SECURITY
The connection to the Mac is a WebSocket to a Tailscale address, which Tailscale encrypts (WireGuard). The pairing link is parsed so that only a loopback or Tailscale address is accepted, and the phone and the Mac verify each other's signatures on every connection. The app therefore carries the NSAllowsArbitraryLoads exception for these non-TLS connections. The only HTTPS request the app can make is the optional dictation model download the person starts in Settings.

ABOUT GUIDELINE 4.2.7
The app is not a remote desktop that mirrors the Mac's screen. It draws native iPhone and iPad views of terminals and agent conversations from structured data. It connects only to a computer the person owns, after the person approved the pairing on that computer. It has no store, no way to buy or browse software, and no account creation.

OTHER
No purchases, no ads, no tracking, no analytics, no data collected (see the privacy policy URL). The agents run on the person's Mac under the person's own accounts with their providers; the app sends nothing to any provider. A demo account is not needed because the app has no accounts.
```

### Rejection risks and mitigations

| # | Risk | Why | Mitigation |
| --- | --- | --- | --- |
| R1 | **2.1 App Completeness: the reviewer cannot get past the pairing screen** | The app needs a Mac with Soyeht and Tailscale; there is no demo mode. | The video and the notes above; TestFlight first; consider the demo mode (item 3). Make the Mac app public before submitting, so the link in the notes works. |
| R2 | **4.2 Minimum Functionality: "a thin web view or a remote control"** | Reviewers may see a companion with nothing of its own. | The app is a native client: its own terminal emulator, agent conversation view, writing editors, Pencil handwriting, dictation, usage rings and iPad board. Say so in the notes (done) and show it in the video. |
| R3 | **4.2.7 Remote Desktop Clients** | Apple's rule for apps that mirror a host asks that the client connect only to a computer the user owns, over a local and LAN-based network, with software fully rendered on the host and no store. This app connects over Tailscale, which is not a LAN, to a computer the user owns, and it draws native views from structured data instead of mirroring a screen. A reviewer may classify it as a remote desktop client anyway. | The notes answer the clause directly (done). Read the current 4.2.7 text in the live guidelines before submitting and adjust the wording. If Apple insists on "local network" only, there is no code fix that keeps the product: the answer is the argument that the tailnet is the person's own private network and that the app is not a mirror. |
| R4 | **ATS exception `NSAllowsArbitraryLoads`** (`Info.plist:39-43`) | A blanket exemption asks for a justification, and a review may ask for it. | The notes carry the justification. The exception may be unneeded: ATS does not apply to a connection to an IP address, and the app connects only to IP literals (`DeviceIdentity.swift:96-107`). Testing the build without it, on a device, would remove the question (finding F3). |
| R5 | **5.1.1 and 5.1.2 data and permissions** | The app asks for camera, microphone, speech recognition and local network. | Each string says what for (section 7 judges them); the notes repeat the reason; the app asks only when the person uses the feature (`PhoneHome.swift:193`, `AppleSpeechRecognizer.swift:15`). The strings F4 proposes name Apple's server processing. |
| R6 | **5.1.2(i) sharing personal data with third-party AI** | A reviewer may ask whether prompts go to an AI service. | They do not go through the app or to the developer. The agents run on the person's Mac under the person's own accounts; the app sends the prompts to the person's own computer only. The notes say so. |
| R7 | **5.2 third-party marks** | The composer shows provider logos (Lobe Icons, MIT) for the harnesses (finding F8). | The listing names no provider. The logos identify the person's own installed tools; keep a line in the notes if asked. |
| R8 | **2.3.10 and 2.3 metadata about other platforms** | Mentioning another store or platform in metadata. | The listing says "Soyeht for Mac" and nothing else; the Windows and Linux apps are not mentioned. |
| R9 | **2.5.2 code execution** | A terminal can look like running code on the device. | The terminal shows a process on the Mac; the app runs and downloads no code (only the optional model weights, data). |
| R10 | **Metadata mismatch: Mac app not yet public** | The notes link the Mac app; a dead link looks like an incomplete submission. | Publish the GitHub release (the `release_flow.py` "make it public" step) before pressing "Submit for Review". |

## 5. Screenshot plan

The agent that wrote this document may not take the screenshots; the owner or a device session
will. The app has no demo mode, so every screenshot is of a real pairing with a Mac that has a
harmless workspace open (a throwaway folder; no real prompts, paths, names or tokens in view).
Use the Dev or the production build with the production name "Soyeht Next" under the icon, so
text that names the app matches the store.

### Sizes (as of 2026)

| Device | Required size (portrait, pixels) | Notes |
| --- | --- | --- |
| iPhone 6.9" (iPhone 16 Pro Max, 17 Pro Max class) | 1320 x 2868 (also accepted: 1290 x 2796, 1260 x 2736) | Required. A set at this size is scaled down for the other iPhone sizes. |
| iPhone 6.5" (iPhone 11 Pro Max, XS Max) | 1284 x 2778 or 1242 x 2688 | Only if you do not upload the 6.9" set. |
| iPad 13" (iPad Pro M4, Air M2 class) | 2064 x 2752 (also accepted: 2048 x 2732) | Required, because the app runs on iPad. |
| iPad 11" | 1488 x 2266, 1668 x 2420, 1668 x 2388 or 1640 x 2360 | Optional; scaled from the 13" set. |

Landscape screenshots are the same sizes with width and height swapped, and for an app like this
the iPad ones should be landscape. One to ten screenshots per size, PNG or JPEG, no transparency.
Apple adds sizes over time; confirm in App Store Connect's "Screenshots" box, which names the size
it wants.

An app preview (a video, 15 to 30 seconds per size) is optional; the demo video of section 4
is longer and goes to App Review, not to the page.

### Which screens, in order

First three screenshots decide the page; show the product working, not the pairing screen.

| # | iPhone | iPad (landscape) | Shows |
| --- | --- | --- | --- |
| 1 | An agent conversation mid-work, with a permission request and its answer buttons | The board: three panes side by side (an agent, a terminal, another agent) with the sidebar | "Your agents, in your hand" |
| 2 | A terminal with the software keyboard and its Esc, Ctrl, Tab and arrow row | A terminal with a hardware keyboard attached, shortcuts menu showing | Real terminals |
| 3 | The workspace list with several panes and their states | Split View with another app beside Soyeht | Many panes |
| 4 | The composer with dictation on | Apple Pencil writing in a terminal or the prompt | Dictation and Pencil |
| 5 | Settings: the usage rings | The new-pane sheet (agent, model, effort, permission mode) | Control |
| 6 | The pairing screen with "Scan Mac QR" and "Use a pairing link" | The same | Private by design: pairing with your own Mac |

Each screenshot may carry a one-line caption added outside the app (plain text on the palette
colours, no extra branding). Do not show: real folder paths, a real tailnet address, a real pairing
code, a provider's login screen, any token, a notification (there are none in 0.1.0), or a device
status bar with a carrier name.

## 6. The owner's checklist, in order

The one thing the owner must have before starting: the Apple Developer Program membership active,
the Apple Distribution certificate in the Mac's keychain, and an App Store Connect API key
(`docs/release.md`, "What you must have").

1. **Certificates, Identifiers and Profiles.** Register the App ID `com.soyeht.next.ios` with the
   **Push Notifications** capability on (the app is signed with `aps-environment`, so a profile
   without it fails the export; finding F5), then make the App Store profile named
   `Soyeht Next iOS App Store` (or set `SOYEHT_IOS_PROFILE`).
2. **Build.** On the release Mac, `uv run python scripts/package_ios.py --validate` (the environment
   variables are in `docs/release.md`). It stops with a validated `.ipa` and does not upload.
3. **App Store Connect, Apps, "+", New App.** Platform iOS; Name (section 1); English (U.S.);
   Bundle ID `com.soyeht.next.ios`; SKU; User access Full. Create.
4. **App Information.** Subtitle; Category Developer Tools, secondary Productivity; Content
   Rights: say the app does not contain or show third-party content (the person's own agents'
   output is not content the developer licenses); the age-rating questionnaire (section 1);
   Privacy Policy URL.
5. **Pricing and Availability.** Free; all territories.
6. **App Privacy.** Get Started, then "No, we do not collect data from this app", then Publish.
7. **Upload the build.** Transporter, or `xcrun altool --upload-app` with the file the script checked.
   Wait for "Processing" to finish (a few minutes) and for the email.
8. **TestFlight.** Add an internal tester, install on a real iPhone and an iPad, pair with a Mac,
   and take the section 5 screenshots from these builds. Then add an external group with the demo
   video link in "Test Information" and submit it for Beta App Review (section 4).
9. **Version page, 0.1.0 (Prepare for Submission).** Upload the screenshots; paste Promotional text,
   Description, Keywords; Support URL and Marketing URL; Copyright; select the build. The
   export-compliance question does not appear, because the key is in the build.
10. **App Review Information.** Contact name, phone and email (the owner's own); Sign-in required
    off; paste the Notes with the two placeholders filled; attach the demo video.
11. **Version Release.** Choose "Manually release this version".
12. **Publish the Mac app first.** The GitHub release must be public (`docs/release.md`, "make it
    public"), so the link in the notes and the "free download" in the description are true.
13. **Add for Review, then Submit for Review.** When approved, press "Release This Version" the same
    day the Mac release goes out.
14. After release, run the checks in `docs/release.md` ("When something goes wrong") and answer any
    review message the same day; a "Metadata Rejected" for a text is fixed in the page and resubmitted
    without a new build.

## 7. Usage-description check

Every permission the code asks for, checked against `apps/ios/Info.plist`:

| Permission | Code that asks | Key in Info.plist | Present | Specific enough |
| --- | --- | --- | --- | --- |
| Camera | `AVCaptureDevice.requestAccess(for: .video)` at `PhoneHome.swift:193`; `DataScannerViewController` at `QRScanner.swift:41` | `NSCameraUsageDescription`, line 30 | yes | **Yes.** "Scan the QR shown by Soyeht on the Mac to pair this iPhone or iPad." says the purpose and the object. |
| Microphone | `AVAudioApplication.requestRecordPermission` at `MicrophoneCapture.swift:20` | `NSMicrophoneUsageDescription`, line 32 | yes | **Yes.** "Dictate messages to your agents: Soyeht listens only while the composer's mic is on." |
| Speech recognition | `SFSpeechRecognizer.requestAuthorization` at `AppleSpeechRecognizer.swift:15` | `NSSpeechRecognitionUsageDescription`, line 34 | yes | **Weak.** It does not say that Apple may process the audio on its servers when the language cannot be recognised on the device (F4a). |
| Local network | tailnet connection through `URLSessionWebSocketTask` at `Wire.swift:78-80` (the system shows the prompt for local addresses; a tailnet address usually does not) | `NSLocalNetworkUsageDescription`, line 36 | yes | **Weak.** "to read pane screens" understates what the connection does: it also sends what the person types and answers (F4b). |
| Notifications | `requestAuthorization` at `PhoneNotifications.swift:92`, only if the engine offers push | none needed (notification permission has no usage string) | n/a | The release engine never offers push; no prompt in 0.1.0. |
| Photos | `PhotosPicker` at `AgentComposerAttachmentViews.swift:105` | none needed (the system picker runs out of process) | n/a | Nothing is read before the person picks. |
| Files | `fileImporter` at `AgentComposerAttachmentViews.swift:110` | none needed | n/a | Same. |
| Tracking, location, contacts, Bluetooth, Face ID, Bonjour | not used (no `AdSupport`, `AppTrackingTransparency`, `CoreLocation`, `Contacts`, `CoreBluetooth`, `LocalAuthentication` import; no `NWBrowser`) | none present | n/a | Do not add keys the code does not use. |

No key is missing. Two are weak. Proposed strings (a product change, so not made in this pull
request):

- F4a, `apps/ios/Info.plist:35` `NSSpeechRecognitionUsageDescription`:
  "Turn what you say into text in the agent composer. Soyeht asks Apple's speech recognition to
  work on this device when your language supports it; otherwise Apple may process the audio."
- F4b, `apps/ios/Info.plist:37` `NSLocalNetworkUsageDescription`:
  "Connect to your own Mac over your private network to show its terminals and agents and to send
  what you type."

## 8. Findings

| # | Severity | Finding | Where | Proposed action |
| --- | --- | --- | --- | --- |
| F1 | High | The app has no demo mode and nothing to see without a paired Mac and a tailnet, so a reviewer cannot reach any feature. | `PhoneHome.swift:137-170` | The video and the notes (section 4); TestFlight first; decide on a sample-workspace mode. |
| F2 | High | Guideline 4.2.7 (remote desktop clients) may be applied: the app connects over Tailscale rather than a LAN. | `DeviceIdentity.swift:96-107`, `PhoneHome.swift:137-170` | The argument in the notes; re-read the live clause; Beta App Review as an early signal. |
| F3 | Medium | `NSAllowsArbitraryLoads` is set for a connection that is to an IP literal, where ATS likely does not apply, so it may be removable and removing it removes a review question. | `apps/ios/Info.plist:39-43`, `docs/contracts/06-mobile.md` section 1 | Test a build without it on a device and a Mac; if it connects, remove it (and the contract text). Otherwise keep the justification in the notes. |
| F4 | Medium | Two usage strings are weak: speech recognition (does not disclose Apple's possible server processing) and local network (understates the use). | `apps/ios/Info.plist:35,37` | The strings in section 7. |
| F5 | Low | `aps-environment` is in the entitlements while push is off. It does no harm to the review, but the App ID needs Push Notifications on or the export fails, and the packager sets it to `production`. | `apps/ios/SoyehtPhone.entitlements:6`, `scripts/package_ios.py` | Enable the capability on the App ID (checklist 1), or remove the entitlement for 0.1.0 and add it back with push. |
| F6 | Low | `docs/apps-iphone.md` says the app "has no encryption of its own". Accurate for confidentiality, but the code uses CryptoKit signatures, hashes and an HMAC for authentication. | `docs/apps-iphone.md`, `DeviceIdentity.swift:151-229` | The wording of section 3 is the precise one; align the sentence when the doc is next touched. |
| F7 | Low | PRIVACY.md does not say that the device's name is sent to the person's Mac at pairing. It stays on the person's own computer and is not "collected", but the policy lists everything that leaves the device. | `PhoneModel.swift:66`, PRIVACY.md | Add one sentence to "What stays on your devices". |
| F8 | Low | The composer shows the logos of four providers (Lobe Icons, MIT licence; the marks belong to their owners). It is a possible 5.2 question. README.md and THIRD-PARTY-NOTICES.txt already credit the icons. | `apps/shared/Sources/SoyehtClient/AgentComposerControlViews.swift:118`, `THIRD-PARTY-NOTICES.txt` | Keep out of the listing (done); answer in the notes if asked. |
| F9 | Info | A fresh checkout builds the Dev bundle identifier; the store identifier lives only in the packager and the export options, not in `contracts/profiles.json`. A hand-made build in Xcode would not be the store app. | `scripts/package_ios.py` (`APP_ID`), `apps/ios/SoyehtPhone.xcodeproj/project.pbxproj` | Always build the store file with `package_ios.py`. |
| F10 | Info | The Keychain service name of the credential is the Dev one in every build, on purpose (so already paired devices do not have to pair again). | `CredentialVault.swift:8-12` | None; a store build still keeps its credential on that device only. |
