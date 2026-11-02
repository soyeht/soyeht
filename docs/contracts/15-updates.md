# Contract 15 · Update notices

Status: v1 (phase 1: notify). Decision: ADR 0122. Code: `soyeht_protocol::update` and
`soyeht_client::update` (the Windows and Linux app), `UpdateCheck` and `UpdateChecker` in
`apps/shared` (the Mac app), `scripts/release_manifest.py` (the release). Values: `contracts/update.json`.

Every build of Soyeht can learn that a newer version is published, from one signed file, and
tell the person. Nothing is downloaded or installed by Soyeht in this version of the contract.

## 1. The published file

Name: `update-manifest.json` (`manifest.asset`), an asset of the GitHub release of the version.
Address: `manifest.url`, which ends in `releases/latest/download/update-manifest.json`.
Larger than `manifest.maxBytes` (65,536) is refused unread.

The file is an envelope:

| Field | Type | Rule |
|---|---|---|
| `format` | whole number | `manifest.format` (1). A boolean, a text or a fraction is not a format. |
| `keyId` | text | The id of one of `signature.publicKeys`. |
| `payload` | text | Padded standard base64 of the payload JSON. Re-encoding what it decodes to gives the same text. |
| `signature` | text | Padded standard base64 of 64 bytes: Ed25519 over `signature.context` followed by the payload bytes. |

Fields the reader does not know are ignored. `signature.context` is `soyeht-update-manifest-v1`
and a newline.

## 2. The payload

| Field | Rule |
|---|---|
| `format` | The same whole number, 1. |
| `channel` | One of `channels` (today `stable`). |
| `sequence` | A whole number from 1 to `manifest.maxSequence` (2^53 - 1) that rises with every manifest published. `release_manifest.py` writes the signing time in seconds. Devices refuse a manifest below the highest one they have read (section 3). |
| `expiresAt` | A whole number of seconds since 1970, above `sequence` and at most `manifest.maxLifetimeSeconds` (7,776,000, 90 days) above it. A reader at or past this time refuses the manifest (section 3), so an old manifest that someone keeps serving does not hold a device on an old release. `release_manifest.py` writes the signing time plus that life. |
| `version` | `MAJOR.MINOR.PATCH`: three plain numbers, none with a leading zero, each below 1,000 (ADR 0115). |
| `minimumSupportedVersion` | The same form, not above `version`. |
| `releaseNotesUrl` | An address below one of `urlPrefixes`. |
| `packages` | 1 to `manifest.maxPackages` (32) entries, each with the five fields below; one entry per system and architecture. |

A package: `system` (one of `systems`: `macos`, `linux`, `windows`, as `std::env::consts::OS`
spells them), `architecture` (one of `architectures`: `aarch64`, `x86_64`, as `ARCH` spells them),
`url` (below a prefix of `urlPrefixes`), `sha256` (64 lowercase hexadecimal digits), `size` (a whole
number from 1 to `manifest.maxPackageBytes`, 4 GiB). Unknown fields are ignored.

An **address** (a package's `url`, `releaseNotesUrl`) is printable ASCII without a space, at most
`manifest.maxAddressBytes` (2,048) bytes, starts with one of `urlPrefixes`, and names a file by a
plain path: it has no backslash, its path (the part before `?` or `#`) holds no `%2f` or `%5c` in
any case, and none of its `/`-separated segments is `.`, `..` or an encoding of either (`%2e`,
`.%2e`, `%2e.`, `%2e%2e`, in any case). A prefix check alone would let `.../releases/../x` leave
the release pages once a server or a browser resolves the path. `sha256` has
`manifest.sha256HexLength` (64) digits and every part of a version is below
`manifest.versionPartLimit` (1,000).

## 3. Reading it

The order is fixed, because the vectors fix the failure each broken manifest ends in:

1. No public key in the build: `notConfigured`, and nothing is fetched.
2. The text is larger than the limit: `tooLarge`.
3. Not JSON, not the envelope, or a `format` that is not the supported one: `badEnvelope`.
4. `keyId` is not a key of the build: `unknownKey`.
5. `payload` or `signature` is not canonical base64, or the signature is not 64 bytes: `badEnvelope`.
6. The signature does not verify under that key: `badSignature`. A scalar at or above the group
   order is refused, and so is a key that is not a point or has an order of 8 or less (with such a
   key the identity point and a zero scalar verify every message). `contracts/update.json` cannot
   hold such a key (`validate_update` checks it), and every reader refuses one anyway, because
   CryptoKit's verifier is not strict: the Mac compares the key with `UpdatePolicy.smallOrderKeys`,
   the desktop asks `is_weak()` and the script computes the order.
7. The payload breaks a rule of section 2: `badManifest`.
8. The payload's `channel` is not the channel the build follows: `wrongChannel`.
9. `sequence` is below the `floor`, the highest sequence this device has read: `staleManifest`.
    An old manifest that someone replays, or a release page that is rolled back, is refused once
    this device has read a newer one. A device that reads a manifest of any outcome (up to date or available) remembers its sequence if it is the highest.
    A re-published manifest keeps its sequence and is read again. A release that must be rolled
    back is published again with a higher `sequence`.
10. The clock is at or past `expiresAt`: `expired`. This comes before the comparison of versions, so
    an expired manifest never reads as "up to date". The reader's clock is the only one it has: a
    device whose clock is far ahead sees the check fail until it is corrected, and a clock before
    1970 reads as the end of time (every manifest expired). A release that is not followed by
    another for 90 days must be published again (a new manifest with a higher `sequence`) before
    the last one expires, or every device's check fails with `expired`.
11. `version` is not above the running version: up to date. A manifest that is older or equal never
   offers anything, whatever else it holds.
12. No package for the build's system and architecture: `noPackage`.
13. Otherwise a newer version is **available**, with that package. It is **required** when the
    running version is below `minimumSupportedVersion`.

A build whose own version is not three plain numbers ends the check as `unknownBuild`. A fetch
that fails is `unreachable` (no network, a status other than 200, a timeout, a redirect away from
HTTPS); both are the client's, not the reader's. The failures are `failures` of
`update.json`; the logs say `kind` with the id, never the text of an error.

The version compared is `MAJOR.MINOR.PATCH` only. A re-upload of one version (the revision of
ADR 0115) is not an update.

## 4. What the build knows

- `UpdatePolicy` (Swift) and `soyeht_protocol::update_policy` (Rust), generated from
  `contracts/update.json`: the address, the limits, the keys, the context, the lists, the
  interval, the texts and the failures. No client types one of these.
- The build's own identity: the product version (`ProductVersion.current`, `CARGO_PKG_VERSION`),
  its system and architecture, and the `defaultChannel`.

## 5. What a check sends

Each check is one HTTPS `GET` of `manifest.url`, which GitHub answers, with a redirect to the file
store of its releases. What leaves the machine:

- the IP address of the connection (the server sees it, as with any request);
- the `User-Agent` `check.userAgent` (`Soyeht-update-check`), the same on both clients, so the
  request names neither the version, nor the system, nor the architecture, nor any identifier of
  the person or the installation. It sends no cookie and no credential;
- the headers the HTTP stack adds on its own: `Host`, `Accept`, and on the Mac `URLSession`'s
  `Accept-Language` and `Accept-Encoding`. The desktop passes `--disable` to curl so that no
  `.curlrc` can add more.

The release is public, so the server cannot know who asked beyond the address. The check is on by
default, and the first request is made at launch without asking: that is an owner decision (ADR
0122), and the switch turns it off.

## 6. When a client asks

At start, then every `check.intervalSeconds` (a day) while it runs, and when the person presses
"Check now". One check runs at a time. The fetch has `check.timeoutSeconds` (20), opens its connection in
`check.connectSeconds` (10) and follows at most `check.maxRedirects` (5), over HTTPS only. The
switch "Check for updates automatically" is on by default, per device (the desktop's local
preferences file `client-preferences.json`, the Mac's user defaults), and when it is off nothing is
fetched and any offer is forgotten. The check does not need the engine.

## 7. What the person sees

The wording is the `texts` of `update.json`, and the sentence of each state is fixed by
`contracts/vectors/update-status.json`:

| State | Sentence |
|---|---|
| switch off | Automatic checks are off. |
| not asked yet | Not checked yet. |
| asking | Checking for updates… |
| nothing newer | Soyeht {version} is up to date. |
| newer | Soyeht {version} is available. (and, when required, This version is no longer supported. Update to keep using Soyeht.) |
| no key in the build | Update checks are not set up in this build. |
| any other failure | Could not check for updates. |

Placement: Preferences, Setup panel on the Windows and Linux app and General panel on the Mac,
an Updates block with the switch, the sentence, "Check now" and, while a newer version is on
offer, "Download Soyeht {version}" and "Release notes", which open the package address and the
notes address in the system browser. While a newer version is on offer the Settings button of the
rail carries the accent dot. No token, color or font is added.

## 8. Writing it

`scripts/release_manifest.py` (see its docstring), called by `scripts/release_flow.py` (ADR 0125, `docs/release.md`):

- `keygen` prints a new signing seed and the `publicKeys` entry for the contract.
- `build` reads `contracts/release.json`, lists `dist/soyeht-<version>-<system>-<architecture>.<extension>`
  (a package of another version in `dist/` stops the build), hashes each, writes and signs
  `dist/update-manifest.json` with `SOYEHT_UPDATE_SIGNING_KEY`, and reads it back as a client does.
  It refuses a key the contract does not carry, because no client would accept it. When a release holds
  a `.deb` and an AppImage, the manifest offers the AppImage; a `.sha256` beside a package is not one.
  The `sequence` is the signing time, and `build` refuses one more than a day ahead of the clock or of
  the last manifest published, or not above that one (`--previous FILE` or `--last-sequence N`), because
  a device never accepts a lower sequence than it has read (section 3, step 9). The recovery is in
  `docs/release.md`.
- `verify FILE [--running X.Y.Z --system S --architecture A]` reads a manifest against the
  contract's keys (or `--trust-key ID=BASE64`) and prints what it holds and what that version would do.

## 9. Replacing a running installation (not in this version)

The rule that phase 2 and a manual update both follow (ADR 0122): never stop the supervisor unless
its program changed; when it changed, say that terminals end and wait for the person's answer;
restart the engine, which reconnects to the supervisor and restores the panes (contract 07).

## 10. Tests

- `contracts/vectors/update.json` (over 60 cases) and `contracts/vectors/update-status.json` (10),
  passed by `soyeht_protocol::update` and `soyeht_client::update` (Rust),
  `UpdateCheckTests`, `UpdateCheckerTests` (Swift) and `contracts/test_update_manifest.py`
  (Python, which also checks the vectors file is current and Ed25519 against RFC 8032).
- `scripts/test_release_manifest.py`: the build, the refusals (the sequence among them), a tampered manifest.
- `apps/shared` `UpdateFetchTests`: the redirect limit and the refusal of a redirect that leaves HTTPS,
  through a `URLProtocol` stub.
- No test reaches the network: the fetch is a function the tests replace, and the desktop's runs
  a stand-in program.
