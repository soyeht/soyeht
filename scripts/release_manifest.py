#!/usr/bin/env python3
"""Build, sign and check the update manifest of a release (ADR 0122).

    uv run python scripts/release_manifest.py keygen
    SOYEHT_UPDATE_SIGNING_KEY=... uv run python scripts/release_manifest.py build [--dist dist]
    uv run python scripts/release_manifest.py verify dist/update-manifest.json [--running 0.1.0]

`build` lists `dist/soyeht-<version>-<system>-<architecture>.<extension>`, hashes each package,
writes the manifest the clients read and signs it with the Ed25519 key in
`SOYEHT_UPDATE_SIGNING_KEY` (the 32-byte seed in base64, as `keygen` prints it). The key never
comes from a file of this repository. The version is `productVersion` of `contracts/release.json`.
`verify` reads a manifest the way a client does, against the keys of `contracts/update.json`
(or `--trust-key ID=BASE64`), so a release can be checked before it is uploaded.

The sequence only grows and a device never accepts a lower one than it has read, so one set too
high locks every later release out of that device. `build` therefore refuses a sequence more than
a day (`MAX_LEAD_SECONDS`) ahead of the clock or of the last one published, and one that is not
above the last one published when that is known (`--previous FILE`, or `--last-sequence N`).
`docs/release.md` says how to recover when a manifest with a sequence too high was published.

A manifest expires `manifest.maxLifetimeSeconds` (90 days) after it is signed, and a device refuses
one past its expiry. Nothing is rebuilt to renew it:

    SOYEHT_UPDATE_SIGNING_KEY=... uv run python scripts/release_manifest.py renew [--input FILE]
    uv run python scripts/release_manifest.py check-expiry [--input FILE] [--warn-days 21]

`renew` reads the manifest the clients read now (the published address, or `--input`), keeps its
version, notes and packages exactly as they are, and signs them again with a new sequence and
expiry. `check-expiry` fails when that manifest expires within `--warn-days` days or has expired.
`docs/release.md` says when to run them.

The manifest is published as an asset of the GitHub release of the same version, under the name
`update-manifest.json`. This script does not upload, tag or build anything.
"""

import argparse
import base64
import hashlib
import json
import os
import re
import secrets
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "contracts"))

import ed25519  # noqa: E402  (the contracts directory is on the path just above)
import product_version  # noqa: E402
import update_manifest as manifest  # noqa: E402

KEY_ENVIRONMENT = "SOYEHT_UPDATE_SIGNING_KEY"
PACKAGE = re.compile(
    r"soyeht-(?P<version>[0-9]+\.[0-9]+\.[0-9]+)-(?P<system>[a-z]+)-(?P<architecture>[a-z0-9_]+)"
    r"\.(?P<extension>[A-Za-z0-9]+(?:\.[A-Za-z0-9]+)*)"
)
# Files that sit beside a package and are not one (`package_mac.py` writes `<file>.sha256`).
SIDECAR_SUFFIXES = (".sha256",)
# A system whose release holds more than one package of an architecture says which one the manifest
# offers, best first (lower case): the AppImage runs on any distribution, the `.deb` on Debian's.
# Any other repeated target is an error.
PREFERRED_EXTENSIONS = {"linux": ("appimage", "deb")}
# The sequence may be at most this far ahead of the clock (and of the last one published).
MAX_LEAD_SECONDS = 86400
# Names the packagers may use for an architecture; the manifest always says the Rust spelling.
ARCHITECTURE_ALIASES = {"arm64": "aarch64", "amd64": "x86_64", "x64": "x86_64"}
POLICY = manifest.POLICY


class Refused(Exception):
    """A problem the person running the script can fix; its message says how."""


def signing_seed(environment):
    text = environment.get(KEY_ENVIRONMENT, "")
    seed = manifest.canonical_base64(text.strip()) if text else None
    if seed is None or len(seed) != 32:
        raise Refused(f"{KEY_ENVIRONMENT} must hold the 32-byte signing seed in base64 (`keygen` makes one)")
    return seed


def key_id_for(seed, keys):
    """The id the contract gives the public half of `seed`; refused when the contract lacks it."""
    public = base64.b64encode(ed25519.public_key(seed)).decode()
    for key in keys:
        if key["key"] == public:
            return key["id"]
    raise Refused(
        "the public half of the signing key is not in contracts/update.json: a client would reject "
        f"the manifest. Add this to signature.publicKeys and regenerate: {{\"id\": \"<name>\", \"key\": \"{public}\"}}"
    )


def preferred(system, first, second):
    """Which of two packages for one target the manifest offers; None when the system has no rule
    that picks between them."""
    order = PREFERRED_EXTENSIONS.get(system, ())
    ranks = [order.index(path.suffix.lstrip(".").lower()) if path.suffix.lstrip(".").lower() in order else None
             for path in (first, second)]
    if None in ranks or ranks[0] == ranks[1]:
        return None
    return first if ranks[0] < ranks[1] else second


def package_files(dist, version):
    """`{(system, architecture): path}` for the packages of `version` in `dist`."""
    found = {}
    for path in sorted(dist.iterdir()):
        match = PACKAGE.fullmatch(path.name)
        if not match or not path.is_file() or path.name.endswith(SIDECAR_SUFFIXES):
            continue
        if match["version"] != version:
            raise Refused(f"{path.name} is a package of another version than {version}; clear dist/ first")
        system = match["system"]
        architecture = ARCHITECTURE_ALIASES.get(match["architecture"], match["architecture"])
        if system not in POLICY["systems"] or architecture not in POLICY["architectures"]:
            raise Refused(f"{path.name}: the system and architecture must be one of "
                          f"{POLICY['systems']} and {POLICY['architectures']}")
        target = (system, architecture)
        if target in found:
            winner = preferred(system, found[target], path)
            if winner is None:
                raise Refused(f"{path.name} and {found[target].name} are for the same system and architecture")
            path = winner
        found[target] = path
    if not found:
        raise Refused(f"no soyeht-{version}-<system>-<architecture>.<extension> package in {dist}")
    return found


def sha256_of(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def check_sequence(sequence, last, now):
    """Refuse a sequence that a device would keep for ever and that would lock later releases out.

    `last` is the sequence of the manifest published before (None when there is none), `now` the
    time in seconds. The ceiling is a day above the later of the two.
    """
    ceiling = max(now, last or 0) + MAX_LEAD_SECONDS
    if sequence > ceiling:
        raise Refused(
            f"sequence {sequence} is {(sequence - max(now, last or 0)) // 3600} hours ahead of "
            f"{'the last published manifest' if last and last > now else 'the clock'}, more than the {MAX_LEAD_SECONDS // 3600} "
            "allowed. A device remembers the highest sequence it has read and refuses every lower one "
            "for ever, so a manifest this far ahead would block the next releases on every device that read it. "
            f"Leave --sequence out (the default is the clock) or give one of at most {ceiling}")
    if last is not None and sequence <= last:
        raise Refused(
            f"sequence {sequence} is not above {last}, the last one published: devices that read that manifest "
            f"would refuse this one as stale. Give --sequence {last + 1} or more (a clock behind the one "
            "that signed the last manifest is the usual cause; docs/release.md, \"When something goes wrong\")")


def build(dist, environment, minimum, keys=None, sequence=None, last_sequence=None, now=None):
    release = product_version.load((ROOT / product_version.RELEASE_SOURCE).read_text())
    version = str(release.version)
    keys = POLICY["signature"]["publicKeys"] if keys is None else keys
    seed = signing_seed(environment)
    key_id = key_id_for(seed, keys)
    templates = POLICY["release"]
    packages = [
        {
            "system": system,
            "architecture": architecture,
            "url": templates["packageUrl"].format(version=version, file=path.name),
            "sha256": sha256_of(path),
            "size": path.stat().st_size,
        }
        for (system, architecture), path in package_files(dist, version).items()
    ]
    # The signing time in seconds: it only grows, and a manifest signed again for the same version
    # (a changed minimum) still outranks the one before it.
    now = int(time.time()) if now is None else now
    sequence = now if sequence is None else sequence
    check_sequence(sequence, last_sequence, now)
    life = POLICY["manifest"]["maxLifetimeSeconds"]
    if sequence + life <= now:
        raise Refused(f"sequence {sequence} is the time of signing, and a manifest signed then expired "
                      f"{life // 86400} days later, already in the past: every device would refuse it. "
                      "Leave --sequence out (the default is the clock)")
    payload = manifest.payload_text(version, minimum, templates["notesUrl"].format(version=version), packages, sequence)
    try:
        manifest.check_payload(payload)
    except manifest.Failed:
        raise Refused("the manifest breaks a rule of the format (contract 15); check the sizes, the "
                      "minimum supported version and the package names") from None
    text = manifest.envelope(payload, seed, key_id)
    # Read it back as a client would, with only the contract's keys.
    manifest.open_envelope(text, keys)
    return text, packages


def describe(text, keys, running, system, architecture, channel):
    payload = manifest.check_payload(manifest.open_envelope(text, keys))
    lines = [f"version {payload['version']} (minimum supported {payload['minimumSupportedVersion']}, "
             f"channel {payload['channel']}, sequence {payload['sequence']})", f"notes {payload['releaseNotesUrl']}"]
    lines += [f"{p['system']}-{p['architecture']}  {p['size']} bytes  {p['sha256']}  {p['url']}" for p in payload["packages"]]
    if running:
        version = manifest.parse_version(running)
        if version is None:
            raise Refused(f"--running must be MAJOR.MINOR.PATCH, not {running!r}")
        lines.append(f"for {running} on {system}-{architecture}: "
                     + json.dumps(manifest.evaluate(text, version, system, architecture, channel, keys,
                                                     now=int(time.time()))))
    return "\n".join(lines)


def previous_sequence(text, keys):
    """The sequence of a manifest published before, read as a client would (its signature is checked)."""
    try:
        return manifest.check_payload(manifest.open_envelope(text, keys))["sequence"]
    except manifest.Failed as failed:
        raise Refused(f"the previous manifest is refused: {failed.failure}") from None


def published(fetch=None):
    """The text of the manifest the clients read now; None when nothing is published."""
    from release_flow import fetch_text  # release_flow imports this module: look it up when needed

    return (fetch or fetch_text)(POLICY["manifest"]["url"])


def read_published(options, fetch=None):
    """The manifest to renew or check: `--input`, else the one published."""
    text = Path(options.input).read_text() if options.input else published(fetch)
    if text is None:
        raise Refused("no manifest is published yet; there is nothing to renew or check")
    return text


def current_payload(text, keys):
    """The payload of a manifest, read for its content only: its expiry and sequence are not
    judged here, because renewing is how an old one is replaced."""
    try:
        return manifest.check_payload(manifest.open_envelope(text, keys))
    except manifest.Failed as failed:
        raise Refused(f"the published manifest is refused: {failed.failure}") from None


def renew(text, environment, keys=None, now=None):
    """The published manifest signed again: the same version, notes and packages (so nothing is
    rebuilt and no hash changes), a new sequence above the old one and a full life."""
    keys = POLICY["signature"]["publicKeys"] if keys is None else keys
    old = current_payload(text, keys)
    seed = signing_seed(environment)
    key_id = key_id_for(seed, keys)
    now = int(time.time()) if now is None else now
    sequence = max(now, old["sequence"] + 1)
    check_sequence(sequence, old["sequence"], now)
    payload = manifest.payload_text(old["version"], old["minimumSupportedVersion"], old["releaseNotesUrl"],
                                    old["packages"], sequence, channel=old["channel"])
    renewed = manifest.envelope(payload, seed, key_id)
    manifest.open_envelope(renewed, keys)
    return renewed


def expiry_problem(text, keys, warn_days, now=None):
    """Why the manifest needs renewing, or None. A manifest has `warn_days` days before it expires."""
    now = int(time.time()) if now is None else now
    expires = current_payload(text, keys)["expiresAt"]
    days = (expires - now) // 86400
    if expires <= now:
        return f"the published manifest expired {(now - expires) // 86400} days ago: every device's update check fails"
    if days < warn_days:
        return f"the published manifest expires in {days} days (less than {warn_days}); renew it"
    return None


def repository():
    """`owner/name` of the public repository, from the release address prefix of the contract."""
    prefix = POLICY["urlPrefixes"][0]
    return prefix.removeprefix("https://github.com/").removesuffix("/releases/")


def release_tag(version):
    """The git tag (and release name) of a version, from the release notes address of the contract."""
    return POLICY["release"]["notesUrl"].rsplit("/", 1)[1].format(version=version)


def trusted_keys(options):
    keys = list(POLICY["signature"]["publicKeys"])
    for item in options:
        key_id, _, key = item.partition("=")
        keys.append({"id": key_id, "key": key})
    return keys


def run(args, environment):
    if args.command == "keygen":
        seed = secrets.token_bytes(32)
        print(base64.b64encode(seed).decode())
        print("Keep the line above secret: it is the value of " + KEY_ENVIRONMENT + ". Never commit it.", file=sys.stderr)
        print("Add this to signature.publicKeys in contracts/update.json, then run contracts/generate.py:", file=sys.stderr)
        print(json.dumps({"id": "release-1", "key": base64.b64encode(ed25519.public_key(seed)).decode()}), file=sys.stderr)
        return
    if args.command == "build":
        dist = Path(args.dist)
        last = args.last_sequence
        if args.previous:
            last = previous_sequence(Path(args.previous).read_text(), trusted_keys([]))
        text, packages = build(dist, environment, args.minimum, sequence=args.sequence, last_sequence=last)
        output = Path(args.output) if args.output else dist / POLICY["manifest"]["asset"]
        output.write_text(text)
        print(f"wrote {output} with {len(packages)} packages")
        return
    if args.command == "renew":
        text = renew(read_published(args), environment)
        output = Path(args.output)
        output.write_text(text)
        print(f"wrote {output}; upload it over the published asset (docs/release.md)")
        return
    if args.command == "check-expiry":
        problem = expiry_problem(read_published(args), trusted_keys([]), args.warn_days)
        if problem:
            raise Refused(problem)
        print("the published manifest is not near its expiry")
        return
    keys = trusted_keys(args.trust_key)
    try:
        print(describe(Path(args.manifest).read_text(), keys, args.running, args.system, args.architecture, args.channel))
    except manifest.Failed as failed:
        raise Refused(f"the manifest is refused: {failed.failure}") from None


def main(argv=None, environment=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("keygen", help="make a signing key and print it")
    made = commands.add_parser("build", help="write and sign the manifest of the packages in dist/")
    made.add_argument("--dist", default=str(ROOT / "dist"))
    made.add_argument("--output", help="where to write it (default: the asset name in --dist)")
    made.add_argument("--sequence", type=int, help="the manifest's sequence number (default: the signing time in "
                      "seconds; it must be above that of the last manifest published and at most a day ahead of "
                      "the clock)")
    made.add_argument("--previous", metavar="FILE", help="the manifest published before, to read its sequence from")
    made.add_argument("--last-sequence", type=int, metavar="N", help="the sequence of the manifest published "
                      "before, when there is no file of it")
    made.add_argument("--minimum", default="0.0.0",
                      help="the oldest version that is still supported (default: every version is)")
    renewed = commands.add_parser("renew", help="sign the published manifest again with a new sequence and expiry, "
                                  "keeping its packages")
    renewed.add_argument("--input", metavar="FILE", help="the manifest to renew (default: the published one)")
    renewed.add_argument("--output", default=POLICY["manifest"]["asset"], help="where to write it")
    expiring = commands.add_parser("check-expiry", help="fail when the published manifest expires soon")
    expiring.add_argument("--input", metavar="FILE", help="the manifest to check (default: the published one)")
    expiring.add_argument("--warn-days", type=int, default=21, help="days before the expiry to fail (default 21)")
    checked = commands.add_parser("verify", help="read a manifest as a client would")
    checked.add_argument("manifest")
    checked.add_argument("--trust-key", action="append", default=[], metavar="ID=BASE64")
    checked.add_argument("--running", help="also say what this version would do with it")
    checked.add_argument("--system", default="macos")
    checked.add_argument("--architecture", default="aarch64")
    checked.add_argument("--channel", default=POLICY["defaultChannel"])
    try:
        run(parser.parse_args(argv), os.environ if environment is None else environment)
    except Refused as refused:
        raise SystemExit(f"release_manifest: {refused}") from None


if __name__ == "__main__":
    main()
