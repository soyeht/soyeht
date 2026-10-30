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
| `version` | `MAJOR.MINOR.PATCH`: three plain numbers, none with a leading zero, each below 1,000 (ADR 0115). |
| `minimumSupportedVersion` | The same form, not above `version`. |
| `releaseNotesUrl` | An address below one of `urlPrefixes`. |
| `packages` | 1 to `manifest.maxPackages` (32) entries, each with the five fields below; one entry per system and architecture. |

A package: `system` (one of `systems`: `macos`, `linux`, `windows`, as `std::env::consts::OS`
spells them), `architecture` (one of `architectures`: `aarch64`, `x86_64`, as `ARCH` spells them),
`url` (below a prefix of `urlPrefixes`), `sha256` (64 lowercase hexadecimal digits), `size` (a whole
number from 1 to `manifest.maxPackageBytes`, 4 GiB). An address is printable ASCII without a space
and at most 2,048 bytes. Unknown fields are ignored.

## 3. Reading it

The order is fixed, because the vectors fix the failure each broken manifest ends in:

1. No public key in the build: `notConfigured`, and nothing is fetched.
2. The text is larger than the limit: `tooLarge`.
3. Not JSON, not the envelope, or a `format` that is not the supported one: `badEnvelope`.
4. `keyId` is not a key of the build: `unknownKey`.
5. `payload` or `signature` is not canonical base64, or the signature is not 64 bytes: `badEnvelope`.
6. The signature does not verify under that key (strictly: a scalar at or above the group order and
   a weak key are refused): `badSignature`.
7. The payload breaks a rule of section 2: `badManifest`.
8. The payload's `channel` is not the channel the build follows: `wrongChannel`.
9. `version` is not above the running version: up to date. A manifest that is older or equal never
   offers anything, whatever else it holds.
10. No package for the build's system and architecture: `noPackage`.
11. Otherwise a newer version is **available**, with that package. It is **required** when the
    running version is below `minimumSupportedVersion`.

A fetch that fails is `unreachable` (no network, a status other than 200, a timeout, a redirect
away from HTTPS); it is the client's, not the reader's. The failures are `failures` of
`update.json`; the logs say `kind` with the id, never the text of an error.

The version compared is `MAJOR.MINOR.PATCH` only. A re-upload of one version (the revision of
ADR 0115) is not an update.

## 4. What the build knows

- `UpdatePolicy` (Swift) and `soyeht_protocol::update_policy` (Rust), generated from
  `contracts/update.json`: the address, the limits, the keys, the context, the lists, the
  interval, the texts and the failures. No client types one of these.
- The build's own identity: the product version (`ProductVersion.current`, `CARGO_PKG_VERSION`),
  its system and architecture, and the `defaultChannel`.

## 5. When a client asks

At start, then every `check.intervalSeconds` (a day) while it runs, and when the person presses
"Check now". The fetch has `check.timeoutSeconds` (20) and follows redirects over HTTPS only. The
switch "Check for updates automatically" is on by default, per device (the desktop's local
preferences file `client-preferences.json`, the Mac's user defaults), and when it is off nothing is
fetched and any offer is forgotten. The check does not need the engine.

## 6. What the person sees

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

## 7. Writing it

`scripts/release_manifest.py` (see its docstring):

- `keygen` prints a new signing seed and the `publicKeys` entry for the contract.
- `build` reads `contracts/release.json`, lists `dist/soyeht-<version>-<system>-<architecture>.<extension>`
  (a package of another version in `dist/` stops the build), hashes each, writes and signs
  `dist/update-manifest.json` with `SOYEHT_UPDATE_SIGNING_KEY`, and reads it back as a client does.
  It refuses a key the contract does not carry, because no client would accept it.
- `verify FILE [--running X.Y.Z --system S --architecture A]` reads a manifest against the
  contract's keys (or `--trust-key ID=BASE64`) and prints what it holds and what that version would do.

## 8. Replacing a running installation (not in this version)

The rule that phase 2 and a manual update both follow (ADR 0122): never stop the supervisor unless
its program changed; when it changed, say that terminals end and wait for the person's answer;
restart the engine, which reconnects to the supervisor and restores the panes (contract 07).

## 9. Tests

- `contracts/vectors/update.json` (46 cases) and `contracts/vectors/update-status.json` (10),
  passed by `soyeht_protocol::update` and `soyeht_client::update` (Rust),
  `UpdateCheckTests`, `UpdateCheckerTests` (Swift) and `contracts/test_update_manifest.py`
  (Python, which also checks the vectors file is current and Ed25519 against RFC 8032).
- `scripts/test_release_manifest.py`: the build, the refusals, a tampered manifest.
- No test reaches the network: the fetch is a function the tests replace, and the desktop's runs
  a stand-in program.
