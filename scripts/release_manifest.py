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
    r"\.(?P<extension>[a-z0-9]+(?:\.[a-z0-9]+)*)"
)
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


def package_files(dist, version):
    """`{(system, architecture): path}` for the packages of `version` in `dist`."""
    found = {}
    for path in sorted(dist.iterdir()):
        match = PACKAGE.fullmatch(path.name)
        if not match or not path.is_file():
            continue
        if match["version"] != version:
            raise Refused(f"{path.name} is a package of another version than {version}; clear dist/ first")
        system = match["system"]
        architecture = ARCHITECTURE_ALIASES.get(match["architecture"], match["architecture"])
        if system not in POLICY["systems"] or architecture not in POLICY["architectures"]:
            raise Refused(f"{path.name}: the system and architecture must be one of "
                          f"{POLICY['systems']} and {POLICY['architectures']}")
        if (system, architecture) in found:
            raise Refused(f"{path.name} and {found[(system, architecture)].name} are for the same system and architecture")
        found[(system, architecture)] = path
    if not found:
        raise Refused(f"no soyeht-{version}-<system>-<architecture>.<extension> package in {dist}")
    return found


def sha256_of(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build(dist, environment, minimum, keys=None, sequence=None):
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
    sequence = int(time.time()) if sequence is None else sequence
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
                     + json.dumps(manifest.evaluate(text, version, system, architecture, channel, keys)))
    return "\n".join(lines)


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
        text, packages = build(dist, environment, args.minimum, sequence=args.sequence)
        output = Path(args.output) if args.output else dist / POLICY["manifest"]["asset"]
        output.write_text(text)
        print(f"wrote {output} with {len(packages)} packages")
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
                      "seconds; it must be above that of the last manifest published)")
    made.add_argument("--minimum", default="0.0.0",
                      help="the oldest version that is still supported (default: every version is)")
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
