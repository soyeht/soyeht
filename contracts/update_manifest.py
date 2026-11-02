"""The signed update manifest: how a release writes one and how a build reads it (ADR 0122).

`contracts/update.json` fixes the limits, the keys and the names; the format is described in
`docs/contracts/15-updates.md`. This is the third reader of `contracts/vectors/update.json`,
after `soyeht_protocol::update` (the desktop client) and `UpdateCheck` (the Mac app), and the
writer `scripts/release_manifest.py` uses. `evaluate` follows their order of checks exactly.

The file published is an envelope, so what is signed is the exact bytes that are read back and no
canonical form of JSON has to be agreed on:

    {"format": 1, "keyId": "...", "payload": "<base64 of the JSON payload>", "signature": "<base64>"}

The signature is Ed25519 over the signing context followed by the payload bytes.
"""

import base64
import binascii
import json
import re
from pathlib import Path

import ed25519
from update import UPDATE_SOURCE, public_key_bytes, validate_update

POLICY = json.loads((Path(__file__).resolve().parent.parent / UPDATE_SOURCE).read_text())
validate_update(POLICY)

VERSION = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
HEX = frozenset("0123456789abcdef")
# A path segment that moves up or stays, in any spelling a browser or a server may read as one.
DOT_SEGMENTS = frozenset({".", "..", "%2e", ".%2e", "%2e.", "%2e%2e"})
PACKAGE_FIELDS = {"system", "architecture", "url", "sha256", "size"}


class Failed(Exception):
    """A check that ends without a verdict; `failure` is an id of `update.json`'s `failures`."""

    def __init__(self, failure):
        super().__init__(failure)
        self.failure = failure


def parse_version(text, policy=POLICY):
    """`MAJOR.MINOR.PATCH` as a tuple, the rule of `contracts/product_version.py`; or None."""
    if not isinstance(text, str) or not VERSION.fullmatch(text):
        return None
    parts = tuple(int(part) for part in text.split("."))
    return parts if max(parts) < policy["manifest"]["versionPartLimit"] else None


def canonical_base64(text):
    """The bytes of padded standard base64; None for anything else, however it might decode."""
    try:
        raw = base64.b64decode(text, validate=True)
    except (binascii.Error, TypeError, ValueError):
        return None
    return raw if base64.b64encode(raw).decode() == text else None


def _format_ok(document, policy):
    """The document is an object whose `format` is the supported whole number (not `true`)."""
    return isinstance(document, dict) and type(document.get("format")) is int and document["format"] == policy["manifest"]["format"]


def signed_bytes(payload, policy=POLICY):
    return policy["signature"]["context"].encode() + payload


def envelope(payload, seed, key_id, policy=POLICY):
    """The text to publish: `payload` (bytes) signed with the key of `seed`."""
    signature = ed25519.sign(seed, signed_bytes(payload, policy))
    return json.dumps(
        {
            "format": policy["manifest"]["format"],
            "keyId": key_id,
            "payload": base64.b64encode(payload).decode(),
            "signature": base64.b64encode(signature).decode(),
        },
        indent=2,
    ) + "\n"


def payload_text(version, minimum, notes_url, packages, sequence, channel=None, policy=POLICY,
                 lifetime=None):
    """The payload for `packages` (dicts with the five package fields), as bytes. It expires
    `lifetime` seconds after `sequence`, the time of signing (at most `maxLifetimeSeconds`)."""
    document = {
        "format": policy["manifest"]["format"],
        "channel": channel or policy["defaultChannel"],
        "sequence": sequence,
        "expiresAt": sequence + (lifetime or policy["manifest"]["maxLifetimeSeconds"]),
        "version": version,
        "minimumSupportedVersion": minimum,
        "releaseNotesUrl": notes_url,
        "packages": packages,
    }
    return json.dumps(document, separators=(",", ":")).encode()


def _address(value, policy):
    """An address below an allowed prefix that names a file by a plain path: printable ASCII, no
    backslash, no encoded slash or backslash, and no segment that moves up or stays."""
    if not (
        isinstance(value, str)
        and len(value) <= policy["manifest"]["maxAddressBytes"]
        and all(0x21 <= ord(c) <= 0x7E for c in value)
        and any(value.startswith(prefix) for prefix in policy["urlPrefixes"])
        and "\\" not in value
    ):
        return False
    path = re.split(r"[?#]", value, maxsplit=1)[0].lower()
    return "%2f" not in path and "%5c" not in path and not DOT_SEGMENTS & set(path.split("/"))


def check_payload(payload, policy=POLICY):
    """The payload as a dict when it keeps every rule of the format; else `Failed("badManifest")`."""
    try:
        document = json.loads(payload)
    except ValueError:
        raise Failed("badManifest") from None
    manifest = policy["manifest"]
    if not _format_ok(document, policy):
        raise Failed("badManifest")
    version = parse_version(document.get("version"), policy)
    minimum = parse_version(document.get("minimumSupportedVersion"), policy)
    packages = document.get("packages")
    sequence = document.get("sequence")
    expires = document.get("expiresAt")
    if (
        document.get("channel") not in policy["channels"]
        or type(sequence) is not int
        or not 0 < sequence <= manifest["maxSequence"]
        or type(expires) is not int
        or not sequence < expires <= sequence + manifest["maxLifetimeSeconds"]
        or version is None
        or minimum is None
        or minimum > version
        or not _address(document.get("releaseNotesUrl"), policy)
        or not isinstance(packages, list)
        or not 0 < len(packages) <= manifest["maxPackages"]
    ):
        raise Failed("badManifest")
    seen = set()
    for package in packages:
        if (
            not isinstance(package, dict)
            or not PACKAGE_FIELDS <= set(package)
            or not isinstance(package["system"], str)
            or not isinstance(package["architecture"], str)
        ):
            raise Failed("badManifest")
        target = (package["system"], package["architecture"])
        size = package["size"]
        if (
            target in seen
            or target[0] not in policy["systems"]
            or target[1] not in policy["architectures"]
            or not _address(package["url"], policy)
            or not isinstance(package["sha256"], str)
            or len(package["sha256"]) != manifest["sha256HexLength"]
            or not HEX.issuperset(package["sha256"])
            or type(size) is not int
            or not 0 < size <= manifest["maxPackageBytes"]
        ):
            raise Failed("badManifest")
        seen.add(target)
    return document


def open_envelope(text, keys, policy=POLICY):
    """The verified payload bytes of `text`, or `Failed`. `keys` is a list of {"id", "key"}."""
    if not keys:
        raise Failed("notConfigured")
    if len(text.encode()) > policy["manifest"]["maxBytes"]:
        raise Failed("tooLarge")
    try:
        document = json.loads(text)
    except ValueError:
        raise Failed("badEnvelope") from None
    fields = ("keyId", "payload", "signature")
    if (
        not _format_ok(document, policy)
        or not all(isinstance(document.get(field), str) for field in fields)
    ):
        raise Failed("badEnvelope")
    key = next((key for key in keys if key["id"] == document["keyId"]), None)
    if key is None:
        raise Failed("unknownKey")
    payload, signature = canonical_base64(document["payload"]), canonical_base64(document["signature"])
    if payload is None or signature is None or len(signature) != 64:
        raise Failed("badEnvelope")
    if not ed25519.verify(public_key_bytes(key["key"]), signed_bytes(payload, policy), signature):
        raise Failed("badSignature")
    return payload


def evaluate(text, running, system, architecture, channel, keys, floor=0, policy=POLICY, *, now):
    """What a build at version `running` (a tuple) should do with the manifest `text`.

    `floor` is the highest sequence this device has seen: a manifest below it is refused, and the
    sequence of one that is read is part of the answer, for the caller to remember. `now` is the
    time in seconds since 1970: a manifest at or past its `expiresAt` is refused, so an old one
    that someone keeps serving does not hold a device on an old release."""
    document = check_payload(open_envelope(text, keys, policy), policy)
    if document["channel"] != channel:
        raise Failed("wrongChannel")
    if document["sequence"] < floor:
        raise Failed("staleManifest")
    if now >= document["expiresAt"]:
        raise Failed("expired")
    version = parse_version(document["version"], policy)
    if version <= running:
        return {"status": "upToDate", "sequence": document["sequence"]}
    package = next(
        (p for p in document["packages"] if (p["system"], p["architecture"]) == (system, architecture)), None
    )
    if package is None:
        raise Failed("noPackage")
    return {
        "status": "available",
        "sequence": document["sequence"],
        "version": document["version"],
        "required": running < parse_version(document["minimumSupportedVersion"], policy),
        "notesUrl": document["releaseNotesUrl"],
        "package": {key: package[key] for key in ("url", "sha256", "size")},
    }


def outcome(text, running, system, architecture, channel, keys, floor=0, policy=POLICY, *, now):
    """`evaluate`, with a failure as a result: what the vectors record."""
    try:
        return evaluate(text, running, system, architecture, channel, keys, floor, policy, now=now)
    except Failed as failed:
        return {"status": "failed", "failure": failed.failure}
