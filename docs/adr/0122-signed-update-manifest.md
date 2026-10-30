# ADR 0122: One signed update manifest, checked by the Mac app and the desktop client

- Status: accepted
- Date: 2026-10-29

## Context

Soyeht has no update mechanism. A person who installed 0.1.0 learns about 0.2.0 only if they look.
The product will be installed from a package on the Mac (ADR 0120), Linux (ADR 0121) and
Windows (ADR 0123), and the version already exists as one contract value (ADR 0115, whose build
number was designed so that a check that compares build numbers agrees with one that compares
versions).

What was asked is the first update path, not the last: tell the person that a newer version is
published, in the Mac app and in the Windows and Linux app, from one source, without a domain, and
without anything a stranger could forge. Installing in place is a second step. iOS is out of
scope: the App Store updates it.

Two facts shaped the decision:

- The public repository `soyeht/soyeht` publishes GitHub releases, and a release asset has a
  stable address that follows the newest release:
  `https://github.com/soyeht/soyeht/releases/latest/download/<asset>`.
- The desktop client has no HTTP library. The engine reaches HTTPS (the usage probes) through the
  system's `curl`, run by `soyeht_process::run` with a deadline and a byte budget.

## Decision

### One manifest, published as a release asset

The release uploads `update-manifest.json` next to the packages. Both clients fetch
`.../releases/latest/download/update-manifest.json`. GitHub's "latest" excludes pre-releases and
drafts, so publishing a release is what makes it visible, and a release can be prepared without
anyone being told. A custom domain can replace the address later by changing one contract value.

The file is an **envelope** around the payload, not a JSON document with a signature field:

    { "format": 1, "keyId": "release-1", "payload": "<base64 of the payload JSON>", "signature": "<base64>" }

The signature is Ed25519 over the signing context (`soyeht-update-manifest-v1`, a line) followed
by the payload's exact bytes. Signing the bytes that are read back means no reader has to agree
with the writer on one exact text for the same data (member order, white space, how a number is
written), which is where
signed-JSON schemes break. The context keeps a signature made for this purpose from being valid
for any other purpose the same key might one day have.

The payload carries `format`, `channel`, `sequence`, `version`, `minimumSupportedVersion`, `releaseNotesUrl`
and a list of `packages` (`system`, `architecture`, `url`, `sha256`, `size`). Its rules, and the
order in which a reader applies them, are in `docs/contracts/15-updates.md`.

### Ed25519, public keys in the contract

Ed25519 is small (32-byte keys, 64-byte signatures), has no parameters to choose wrongly, is
deterministic, and is in the platforms' own libraries (CryptoKit on the Mac) or one crate that the
engine workspace already uses (`ed25519-dalek`). The public keys are `signature.publicKeys` of
`contracts/update.json`, each with an id, generated into Rust and Swift. A list, not a key, so a
key can be rotated: the new key ships in a release signed by the old one, and the old key is
removed in a later release. The private half never enters the repository: the release script
reads it from the environment (`SOYEHT_UPDATE_SIGNING_KEY`).

**Until a release key exists the list is empty, and a build with no key checks nothing**: it does
not fetch, and says "Update checks are not set up in this build". Shipping with a placeholder key
would make every check pass or fail for no reason; shipping with none fails closed and visibly.

### Where the logic lives, and why three readers pass one vector file

Reading a manifest is a security rule, so it exists once per language and is held together by
data. The desktop client uses `soyeht_protocol::update` (layer 2, pure: no IO, no clock, no
runtime, which is what that crate is for). The Mac app uses `UpdateCheck` in `apps/shared`.
`contracts/update_manifest.py` is the third reader, used by the release script. All three pass
`contracts/vectors/update.json`: over 60 cases that include a tampered payload, a flipped signature
bit, a signature whose scalar was raised by the group order (the malleable form), a manifest
signed without the context, a key the build does not carry, a second trusted key, every rule of
the payload, and the choice of package by system and architecture. The vectors are generated
from throwaway test keys by `contracts/test_update_manifest.py --write`, which also proves the
file is current, and the Python Ed25519 behind them is checked against RFC 8032. The Rust and
Swift implementations verify what Python signed, so the three agree on the signature as well as
on the rules.

The wording the clients show is a `texts` section of `update.json`, generated for both, and the
sentence for each state is held by `contracts/vectors/update-status.json`.

### A check per client, not one in the engine

The engine already has HTTPS and could answer "is there an update?" over the protocol, with one
implementation and no Swift crypto. It was rejected: the engine is the thing an update replaces
and the thing that may be broken or absent when an update is wanted; the check must not depend on
the connection that is healed by the update. It would also put a periodic outbound request and a
wire method in the engine for a client concern, and the wire has to stay stable across the very
versions that are being replaced.

So each client fetches. The Mac uses `URLSession` (HTTPS only, the system's trust, a 20 s limit,
at most the manifest's size). The desktop client runs `curl` through `soyeht_process::run`, as the
engine does for its probes: HTTPS only, at most five redirects (a release asset address redirects
to the file), a size and a time limit. This adds no HTTP crate to the desktop workspace. It adds
a dependency of `soyeht-client` on `soyeht-process` (a sibling of `soyeht-platform` in the same
layer), which `check_layers.py` and `AGENTS.md` now list. The consequence is that the Linux
package depends on `curl`, which every desktop distribution ships and Windows has shipped since
Windows 10. A machine without it sees "Could not check for updates".

The transport is not trusted at all. The signature is what makes the manifest count, so a
captive portal, a proxy or a bad certificate store can only make a check fail.

### What a check sends, and what the owner decides

A check is one HTTPS `GET`. The server (GitHub, and the file store its release assets redirect
to) sees the IP address of the connection and one `User-Agent`, `Soyeht-update-check` (a value of
`update.json`, set by both clients, so the request names neither the version, nor the system, nor
the architecture), plus the headers the HTTP stack adds itself: `Host` and `Accept` everywhere,
`Accept-Language` and `Accept-Encoding` from `URLSession` on the Mac. No cookie, no identifier of
the person or the installation, no credential. The desktop passes `--disable` to `curl`, so a
`.curlrc` cannot add anything or change the output. Without a version in the request the server
learns nothing about which release a machine runs; the price is that GitHub's counts cannot tell
versions apart by this request.

These are **owner decisions**, taken here as the defaults and open to change:

1. The check is **on by default**, and the first request is made at launch without asking.
   (The alternative, off until the person opts in, means most installs never learn of an update.)
2. The request carries no version, system or identifier (above). The decision is made on the device, so
   the server has no need to know which versions are installed, and does not.
3. The channel is `stable` only, from the repository's releases, with no domain of ours yet.

### When it runs, and the setting

At start, and then each day (`check.intervalSeconds`) while the app runs. The choice "Check for
updates automatically" is on by default and is kept **per device** (the desktop's local
preferences file, the Mac's user defaults), not in the engine's settings: it is about this
application, it must work when the engine does not, and the engine's settings exist for what
every client of one engine shares. Turning it off stops the daily check and forgets any offer,
so the badge goes away. "Check now" asks at once.

### What the person sees, the same on both

- In Preferences, under Setup (the Mac's General panel), an **Updates** block: the switch, one
  sentence ("Soyeht 0.2.0 is available.", "Soyeht 0.1.0 is up to date.", "Could not check for
  updates.", ...), a "Check now" button, and, when a newer version exists, "Download Soyeht
  0.2.0" and "Release notes", which open the addresses in the person's browser.
- A subtle dot on the Settings button of the rail, the accent dot that shows a working agent, only
  while a newer version is on offer.
- A version below `minimumSupportedVersion` gets one more line saying it is no longer supported.
  It does not block anything: there is nothing to block with before an installer exists.

Palette roles, spacing and type sizes are the existing ones; the block adds no token, color or
font.

### Phase 1 only notifies; the rule for phase 2 is set now

Phase 1 downloads nothing: the browser downloads the package from the release page. The manifest
already carries `sha256` and `size` of each package so that phase 2 can verify a download without
changing the format. The addresses are restricted to the repository's release pages
(`urlPrefixes`), so a signed manifest cannot send the button anywhere else.

**The rule for replacing a running installation (phase 2, and for a person who updates by hand
today).** The engine and the supervisor outlive the apps (contract 07, ADR 0108): the supervisor
holds every pty, so terminals, harness processes and language servers die with it.

1. An update replaces the apps and the engine, and **never stops the supervisor** unless the
   supervisor's own program changed. The engine restarts and reconnects to the supervisor, which
   restores every pane; that is the designed path and it keeps the terminals.
2. When the supervisor's program changed (its build id differs from the running one), the update
   does not replace it silently. The client says that updating ends the running terminals, lists
   how many panes are open, and waits for the person to confirm. The update installs the new
   supervisor only after that answer, and starts it with the engine (the service seam of ADR 0114
   restarts both). A person who declines installs everything except the supervisor, which then
   keeps running its old program until the next time they choose to restart it.
3. The new engine must speak to the running supervisor of the previous build. The supervisor's
   protocol has a version of its own, independent of the client's (ADR 0003); a release that
   changes it counts as a supervisor change for this rule.
4. The order is: verify the package (`sha256`, `size`), install the apps, restart the engine,
   and only then, if confirmed, the supervisor. A failure before the engine restarts leaves the
   old installation running.

### Mac in-place install: Sparkle 2 or our own

Evaluated for phase 2, not adopted yet. Sparkle 2 downloads, verifies its own EdDSA signature,
replaces the bundle atomically from a helper that survives the app's exit, shows its own UI, and
handles authorization and relaunch. Writing that safely (a replacing helper, translocation,
quarantine, rollback) is a long tail that Sparkle has already paid. Its cost is a third-party
binary framework in a signed and notarized app, an `appcast.xml` of its own beside our manifest,
and its UI, which is not the palette's. **Decision for phase 2: adopt Sparkle on the Mac, and
keep the manifest as the single source.** Sparkle takes an appcast; the release script generates
it from the same manifest and the same packages (same version, same files, same size), and the
Sparkle EdDSA key is a second signing key kept like the first. The manifest remains what the
Linux and Windows clients read and what the Mac's notice reads; Sparkle is only the installer for
the Mac package. Sparkle must be told to install the engine-and-supervisor rule above (its
`SPUUpdaterDelegate` can refuse an install, which is where the supervisor confirmation lives).
Linux and Windows have no such framework worth the weight; their phase 2 is a small installer
step of our own, the packagers' business, that follows the same rule. If Sparkle later proves
heavier than its worth, the Mac falls back to the same own step; nothing in the manifest changes.

This is a decision for later and costs nothing now. Phase 1 adds no dependency to the Mac.

## Consequences

- A release is four steps: bump the version (ADR 0115), build the packages into `dist/`, run
  `uv run python scripts/release_manifest.py build` with the signing key in the environment, and
  upload `dist/` including `update-manifest.json` to a GitHub release. `verify` reads a manifest as
  a client does before it is uploaded.
- Someone must create the key and put its public half in `contracts/update.json`
  (`release_manifest.py keygen` prints both). Until then no build checks. The private half is the
  one secret of the update path: a copy on the release machine's keychain and a second sealed
  copy elsewhere, since a lost key means a release no installed copy will accept and every
  installed copy needs a build that carries the new key, delivered by hand.
- A leaked private key cannot be recalled. The remedy is a release signed by the old key that
  carries the new public key and no longer lists the old one; the installed copies that took it
  stop trusting the leaked key. A copy that did not update stays exposed.
- **Replay and freeze.** The payload carries a `sequence` that rises with every manifest
  (`release_manifest.py` writes the signing time in seconds), and each device remembers the
  highest it has read (the desktop in its local preferences file, the Mac in its user defaults)
  and refuses a manifest below it. Someone who can serve old files can still hide a new release
  from a device that has not yet read it (a freeze), but cannot hand it an older manifest after
  it has read a newer one, cannot offer a version that was not signed, and cannot make a copy go
  backwards: only a version above the running one is offered. Publishing a manifest with a lower
  sequence than the last one locks every device that read the last; correct a release by
  publishing it again with a higher sequence. A signing machine with a wrong clock far ahead has
  the same effect, so `--sequence` can set it by hand. The rule is required before phase 2.
- **Keys of small order.** CryptoKit has no strict verification, and a public key that is the
  identity point verifies every message with the identity point as its signature. A key that is
  not a point or has an order of 8 or less is refused by `validate_update` (so it can never be
  committed) and again by every reader (the desktop through `is_weak()`, the Mac against the list
  generated into `UpdatePolicy.smallOrderKeys`, the script by computing the order), with a vector.
- A packager must name files `dist/soyeht-<version>-<system>-<architecture>.<extension>` with
  `system` in `macos|linux|windows` and `architecture` in `aarch64|x86_64` (the spelling of
  Rust's `std::env::consts`; `arm64`, `amd64` and `x64` are accepted and rewritten).
- Automated runs of the desktop client reach GitHub once at start once a key exists. A test that
  must not should turn the check off in its preferences file.
- `soyeht-protocol` gains `ed25519-dalek` (already in the engine workspace), and the desktop
  workspace's lockfile gains it with its small dependency tree. Both are reviewed changes.
- The dot reuses the shape of the working-agent dot; if the two ever need to be told apart, that
  is a design decision, not a layer problem.
