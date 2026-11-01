"""Tests of the update contract, the Python reader of the manifest and `contracts/vectors/update.json`.

Run `uv run python contracts/test_update_manifest.py --write` to rewrite the vectors after a
change to the format; the Rust and Swift tests read the same file. The keys below are throwaway
test keys derived from a phrase, and sign nothing but these vectors.
"""

import base64
import copy
import hashlib
import json
import sys
import unittest
from pathlib import Path

import ed25519
import update
import update_manifest as manifest
from update import render_update, validate_update

ROOT = Path(__file__).resolve().parent
VECTORS = ROOT / "vectors" / "update.json"
FIRST = hashlib.sha256(b"soyeht test update key one").digest()
SECOND = hashlib.sha256(b"soyeht test update key two").digest()
IDENTITY = b"\x01" + b"\x00" * 31
KEYS = {
    "weak": {"id": "test-weak", "key": base64.b64encode(IDENTITY).decode()},
    "first": {"id": "test-1", "key": base64.b64encode(ed25519.public_key(FIRST)).decode()},
    "second": {"id": "test-2", "key": base64.b64encode(ed25519.public_key(SECOND)).decode()},
}
SEQUENCE = 1_000
LIFETIME = manifest.POLICY["manifest"]["maxLifetimeSeconds"]
# The time a case reads the manifest at, unless it says: just after it was signed.
NOW = SEQUENCE + 1
UP_TO_DATE = {"status": "upToDate", "sequence": SEQUENCE}
NOTES = "https://github.com/soyeht/soyeht/releases/tag/v0.2.0"
BASE = "https://github.com/soyeht/soyeht/releases/download/v0.2.0/"


def package(system, architecture, extension, **changes):
    name = f"soyeht-0.2.0-{system}-{architecture}.{extension}"
    made = {
        "system": system,
        "architecture": architecture,
        "url": BASE + name,
        "sha256": hashlib.sha256(name.encode()).hexdigest(),
        "size": 40_000_000 + len(name),
    }
    return {**made, **changes}


PACKAGES = [
    package("macos", "aarch64", "dmg"),
    package("macos", "x86_64", "dmg"),
    package("linux", "x86_64", "tar.gz"),
    package("windows", "x86_64", "zip"),
]


def payload(**changes):
    document = {
        "format": 1,
        "channel": "stable",
        "sequence": SEQUENCE,
        "expiresAt": SEQUENCE + LIFETIME,
        "version": "0.2.0",
        "minimumSupportedVersion": "0.1.0",
        "releaseNotesUrl": NOTES,
        "packages": copy.deepcopy(PACKAGES),
    }
    document.update(changes)
    return json.dumps(document, separators=(",", ":")).encode()


def signed(body, seed=FIRST, key_id="test-1"):
    return manifest.envelope(body, seed, key_id)


def tampered(text, field, change):
    document = json.loads(text)
    document[field] = change(document[field])
    return json.dumps(document, indent=2) + "\n"


def flip_last_byte(encoded):
    raw = bytearray(base64.b64decode(encoded))
    raw[-1] ^= 1
    return base64.b64encode(bytes(raw)).decode()


def malleable(encoded):
    """The same signature with the scalar raised by the group order: a second valid-looking form."""
    raw = base64.b64decode(encoded)
    scalar = int.from_bytes(raw[32:], "little") + ed25519.L
    return base64.b64encode(raw[:32] + scalar.to_bytes(32, "little")).decode()


def case(name, text, expected, running="0.1.0", system="macos", architecture="aarch64", channel="stable",
         trust=("first",), floor=0, now=NOW):
    return {
        "name": name,
        "running": running,
        "system": system,
        "architecture": architecture,
        "channel": channel,
        "trust": list(trust),
        "floor": floor,
        "now": now,
        "manifest": text,
        "expected": expected,
    }


def weak_signed():
    """A manifest "signed" by the identity point: it verifies under a lenient check, for any text."""
    return json.dumps(
        {
            "format": 1,
            "keyId": "test-weak",
            "payload": base64.b64encode(payload()).decode(),
            "signature": base64.b64encode(IDENTITY + bytes(32)).decode(),
        }
    )


def failed(failure):
    return {"status": "failed", "failure": failure}


def available(version="0.2.0", required=False, index=0):
    chosen = PACKAGES[index]
    return {
        "status": "available",
        "sequence": SEQUENCE,
        "version": version,
        "required": required,
        "notesUrl": NOTES,
        "package": {key: chosen[key] for key in ("url", "sha256", "size")},
    }


def build_cases():
    good = signed(payload())
    unsigned_context = json.dumps(
        {
            "format": 1,
            "keyId": "test-1",
            "payload": base64.b64encode(payload()).decode(),
            "signature": base64.b64encode(ed25519.sign(FIRST, payload())).decode(),
        }
    )
    cases = [
        case("a newer version is offered with the package of this system", good, available()),
        case("the same version is up to date", good, UP_TO_DATE, running="0.2.0"),
        case("a newer build than the manifest is up to date", good, UP_TO_DATE, running="0.10.0"),
        case("a version below the minimum supported is required", signed(payload(minimumSupportedVersion="0.2.0")),
             available(required=True)),
        case("a version at the minimum supported is not required", signed(payload(minimumSupportedVersion="0.1.0")),
             available(required=False)),
        case("an Intel Mac gets its own package", good, available(index=1), architecture="x86_64"),
        case("Linux gets its own package", good, available(index=2), system="linux", architecture="x86_64"),
        case("Windows gets its own package", good, available(index=3), system="windows", architecture="x86_64"),
        case("a system with no package is told so", good, failed("noPackage"), system="windows", architecture="aarch64"),
        case("up to date wins over a missing package", good, UP_TO_DATE, running="0.2.0",
             system="windows", architecture="aarch64"),
        case("versions compare as numbers, not text", signed(payload(version="0.10.0", minimumSupportedVersion="0.9.0")),
             available(version="0.10.0"), running="0.9.0"),
        case("another channel is refused", good, failed("wrongChannel"), channel="beta"),
        case("a build with no key trusts nothing", good, failed("notConfigured"), trust=()),
        case("a payload changed after signing is refused", tampered(good, "payload", lambda p: base64.b64encode(
            base64.b64decode(p).replace(b'"0.2.0"', b'"9.9.9"', 1)).decode()), failed("badSignature")),
        case("a signature with one bit changed is refused", tampered(good, "signature", flip_last_byte), failed("badSignature")),
        case("a signature with the scalar raised by the group order is refused",
             tampered(good, "signature", malleable), failed("badSignature")),
        case("a manifest signed without the context is refused", unsigned_context, failed("badSignature")),
        case("a manifest signed by another key under the trusted id is refused",
             signed(payload(), seed=SECOND, key_id="test-1"), failed("badSignature")),
        case("a key id this build does not carry is refused", signed(payload(), seed=SECOND, key_id="test-2"), failed("unknownKey")),
        case("a second trusted key is accepted", signed(payload(), seed=SECOND, key_id="test-2"), available(),
             trust=("first", "second")),
        case("a key of the identity point verifies nothing, whatever the signature", weak_signed(),
             failed("badSignature"), trust=("weak",)),
        case("a manifest as new as the newest seen is read", good, available(), floor=SEQUENCE),
        case("a manifest older than one already seen is refused", good, failed("staleManifest"), floor=SEQUENCE + 1),
        case("an older manifest is refused even when it offers nothing newer", good, failed("staleManifest"),
             running="0.2.0", floor=SEQUENCE + 1),
        case("a newer sequence is read", signed(payload(sequence=SEQUENCE + 5, expiresAt=SEQUENCE + 5 + LIFETIME)),
             {**available(), "sequence": SEQUENCE + 5}, floor=SEQUENCE),
        case("a manifest is read up to the second before it expires", good, available(), now=SEQUENCE + LIFETIME - 1),
        case("a manifest is refused at its expiry", good, failed("expired"), now=SEQUENCE + LIFETIME),
        case("a manifest long past its expiry is refused", good, failed("expired"), now=SEQUENCE + 10 * LIFETIME),
        case("an expired manifest never reads as up to date", good, failed("expired"), running="0.2.0",
             now=SEQUENCE + LIFETIME),
        case("a stale sequence is reported before an expiry", good, failed("staleManifest"), floor=SEQUENCE + 1,
             now=SEQUENCE + LIFETIME),
        case("a wrong channel is reported before an expiry", good, failed("wrongChannel"), channel="beta",
             now=SEQUENCE + LIFETIME),
        case("another channel is refused before the sequence is compared", good, failed("wrongChannel"),
             channel="beta", floor=SEQUENCE + 1),
        case("text that is not JSON is not an envelope", "not json", failed("badEnvelope")),
        case("an envelope of another format is refused", tampered(good, "format", lambda _: 2), failed("badEnvelope")),
        case("a boolean is not a format", tampered(good, "format", lambda _: True), failed("badEnvelope")),
        case("an envelope with no signature is refused", json.dumps({k: v for k, v in json.loads(good).items() if k != "signature"}),
             failed("badEnvelope")),
        case("a short signature is refused", tampered(good, "signature", lambda s: base64.b64encode(base64.b64decode(s)[:63]).decode()),
             failed("badEnvelope")),
        case("a payload that is not canonical base64 is refused", tampered(good, "payload", lambda p: p.rstrip("=")),
             failed("badEnvelope")),
    ]
    broken = {
        "a version with two parts": payload(version="0.2"),
        "a version with a leading zero": payload(version="0.02.0"),
        "a version part of 1000": payload(version="0.1000.0"),
        "a minimum above the version": payload(minimumSupportedVersion="0.3.0"),
        "a channel the contract does not list": payload(channel="nightly"),
        "a payload of another format": payload(format=2),
        "no sequence": json.dumps({**json.loads(payload()), "sequence": None}).encode(),
        "a sequence of 0": payload(sequence=0),
        "a negative sequence": payload(sequence=-1),
        "a sequence that is text": payload(sequence="1000"),
        "a fractional sequence": json.dumps({**json.loads(payload()), "sequence": 1000.5}).encode(),
        "a sequence above the limit": payload(sequence=manifest.POLICY["manifest"]["maxSequence"] + 1),
        "no expiry": json.dumps({k: v for k, v in json.loads(payload()).items() if k != "expiresAt"}).encode(),
        "an expiry that is text": payload(expiresAt=str(SEQUENCE + LIFETIME)),
        "a fractional expiry": json.dumps({**json.loads(payload()), "expiresAt": SEQUENCE + 5.5}).encode(),
        "an expiry that is the time of signing": payload(expiresAt=SEQUENCE),
        "an expiry before the time of signing": payload(expiresAt=SEQUENCE - 1),
        "an expiry beyond the longest life": payload(expiresAt=SEQUENCE + LIFETIME + 1),
        "a notes address with a dot segment": payload(releaseNotesUrl=BASE + "../../../evil/x"),
        "a package address going up a folder": payload(
            packages=[package("macos", "aarch64", "dmg", url=BASE + "../v0.1.0/soyeht.dmg")]),
        "a package address with an encoded dot segment": payload(
            packages=[package("macos", "aarch64", "dmg", url=BASE + "%2E%2e/other/soyeht.dmg")]),
        "a package address with a half encoded dot segment": payload(
            packages=[package("macos", "aarch64", "dmg", url=BASE + ".%2e/other/soyeht.dmg")]),
        "a package address with a lone encoded dot": payload(
            packages=[package("macos", "aarch64", "dmg", url=BASE + "%2e/soyeht.dmg")]),
        "a package address with an encoded slash": payload(
            packages=[package("macos", "aarch64", "dmg", url=BASE + "..%2Fother/soyeht.dmg")]),
        "a package address with a backslash": payload(
            packages=[package("macos", "aarch64", "dmg", url=BASE + "..\\other\\soyeht.dmg")]),
        "a notes address that is not https": payload(releaseNotesUrl="http://github.com/soyeht/soyeht/releases/tag/v0.2.0"),
        "a notes address outside the release pages": payload(releaseNotesUrl="https://example.com/soyeht/releases/"),
        "no packages": payload(packages=[]),
        "a package address outside the release pages": payload(
            packages=[package("macos", "aarch64", "dmg", url="https://example.com/soyeht.dmg")]),
        "a package address with a space": payload(
            packages=[package("macos", "aarch64", "dmg", url=BASE + "soyeht 0.2.0.dmg")]),
        "a checksum in capitals": payload(packages=[package("macos", "aarch64", "dmg", sha256="A" * 64)]),
        "a checksum that is too short": payload(packages=[package("macos", "aarch64", "dmg", sha256="a" * 63)]),
        "a package of no bytes": payload(packages=[package("macos", "aarch64", "dmg", size=0)]),
        "a package size that is text": payload(packages=[package("macos", "aarch64", "dmg", size="12")]),
        "a package larger than the limit": payload(
            packages=[package("macos", "aarch64", "dmg", size=manifest.POLICY["manifest"]["maxPackageBytes"] + 1)]),
        "a system the contract does not list": payload(packages=[package("solaris", "aarch64", "pkg")]),
        "an architecture the contract does not list": payload(packages=[package("macos", "riscv64", "dmg")]),
        "one system and architecture twice": payload(packages=[package("macos", "aarch64", "dmg")] * 2),
        "text that is not JSON": b"not json",
    }
    cases += [case(f"{name} is refused after the signature checks out", signed(body), failed("badManifest"))
              for name, body in broken.items()]
    return cases


def build_vectors():
    return {
        "description": (
            "The signed update manifest read by the desktop client (soyeht_protocol::update), the Mac app "
            "(UpdateCheck) and the release script (contracts/update_manifest.py). `trust` names the keys "
            "of `keys` a build carries; `expected` is the result of reading `manifest` as version "
            "`running` on `system` and `architecture` in `channel`, when the highest sequence this device has seen is "
            "`floor`, at `now` (seconds since 1970). The keys are throwaway test keys. "
            "Regenerate with: uv run python contracts/test_update_manifest.py --write"
        ),
        "keys": KEYS,
        "cases": build_cases(),
    }


def read(case_):
    keys = [KEYS[name] for name in case_["trust"]]
    return manifest.outcome(case_["manifest"], manifest.parse_version(case_["running"]), case_["system"],
                            case_["architecture"], case_["channel"], keys, case_["floor"], now=case_["now"])


class Ed25519Tests(unittest.TestCase):
    def test_the_signatures_of_rfc_8032(self):
        # RFC 8032 section 7.1, tests 1 and 2.
        first = bytes.fromhex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
        self.assertEqual(ed25519.public_key(first).hex(), "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
        signature = ed25519.sign(first, b"")
        self.assertEqual(signature.hex(), "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b")
        self.assertTrue(ed25519.verify(ed25519.public_key(first), b"", signature))
        second = bytes.fromhex("4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb")
        self.assertEqual(ed25519.public_key(second).hex(), "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c")
        self.assertEqual(ed25519.sign(second, b"\x72").hex(), "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00")

    def test_a_changed_message_or_key_does_not_verify(self):
        public = ed25519.public_key(FIRST)
        signature = ed25519.sign(FIRST, b"message")
        self.assertFalse(ed25519.verify(public, b"messagf", signature))
        self.assertFalse(ed25519.verify(ed25519.public_key(SECOND), b"message", signature))
        self.assertFalse(ed25519.verify(public, b"message", signature[:63]))


class ContractTests(unittest.TestCase):
    def setUp(self):
        self.document = copy.deepcopy(manifest.POLICY)

    def test_the_contract_renders_for_both_clients(self):
        outputs = render_update(self.document)
        self.assertEqual(set(outputs), {update.RUST_OUTPUT, update.SWIFT_OUTPUT})
        self.assertIn("pub enum Failure", outputs[update.RUST_OUTPUT])
        self.assertIn("public enum UpdateFailure", outputs[update.SWIFT_OUTPUT])

    def test_a_key_is_32_bytes_of_canonical_base64(self):
        for bad in ("", "AAAA", base64.b64encode(b"x" * 31).decode(), base64.b64encode(b"x" * 33).decode()):
            self.document["signature"]["publicKeys"] = [{"id": "k1", "key": bad}]
            with self.assertRaises(ValueError, msg=bad):
                validate_update(self.document)
        self.document["signature"]["publicKeys"] = [KEYS["first"], KEYS["first"]]
        with self.assertRaises(ValueError):
            validate_update(self.document)

    def test_a_key_that_is_not_a_point_or_has_a_small_order_can_never_be_committed(self):
        order_two = (2**255 - 20).to_bytes(32, "little")
        for weak in (IDENTITY, bytes(32), order_two, bytes([0xFF] * 32), *ed25519.small_order_encodings()):
            self.document["signature"]["publicKeys"] = [{"id": "k1", "key": base64.b64encode(weak).decode()}]
            with self.assertRaises(ValueError, msg=weak.hex()):
                validate_update(self.document)
        self.document["signature"]["publicKeys"] = [KEYS["first"], KEYS["second"]]
        validate_update(self.document)

    def test_the_small_order_forms_are_what_a_strict_check_would_refuse(self):
        forms = ed25519.small_order_encodings()
        self.assertIn(IDENTITY, forms)
        self.assertTrue(all(ed25519.is_weak(raw) for raw in forms))
        self.assertFalse(any(ed25519.is_weak(ed25519.public_key(seed)) for seed in (FIRST, SECOND)))
        self.assertTrue(ed25519.verify(ed25519.public_key(FIRST), b"x", ed25519.sign(FIRST, b"x")))
        # The identity key and a zero signature "verify" every message without the weak-key rule.
        self.assertFalse(ed25519.verify(IDENTITY, b"anything", IDENTITY + bytes(32)))

    def test_the_manifest_url_is_below_an_allowed_prefix(self):
        self.document["urlPrefixes"] = ["https://example.com/"]
        with self.assertRaises(ValueError):
            validate_update(self.document)

    def test_a_failure_is_listed_once_and_has_a_meaning(self):
        self.document["failures"].append(dict(self.document["failures"][0]))
        with self.assertRaises(ValueError):
            validate_update(self.document)

    def test_the_default_channel_is_a_channel(self):
        self.document["defaultChannel"] = "beta"
        with self.assertRaises(ValueError):
            validate_update(self.document)


class VectorTests(unittest.TestCase):
    def test_the_vectors_file_is_current(self):
        self.assertEqual(json.loads(VECTORS.read_text()), build_vectors(), "run this file with --write")

    def test_every_case_reads_as_recorded(self):
        for case_ in json.loads(VECTORS.read_text())["cases"]:
            self.assertEqual(read(case_), case_["expected"], case_["name"])

    def test_every_failure_of_the_contract_that_a_manifest_can_cause_is_in_the_vectors(self):
        recorded = {c["expected"].get("failure") for c in json.loads(VECTORS.read_text())["cases"]}
        needs_transport = {"unreachable", "tooLarge", "unknownBuild"}
        listed = {f["id"] for f in manifest.POLICY["failures"]}
        self.assertEqual(listed - needs_transport, recorded - {None})

    def test_a_manifest_over_the_limit_is_refused_before_it_is_read(self):
        text = signed(payload()) + " " * manifest.POLICY["manifest"]["maxBytes"]
        with self.assertRaises(manifest.Failed) as caught:
            manifest.evaluate(text, (0, 1, 0), "macos", "aarch64", "stable", [KEYS["first"]], now=NOW)
        self.assertEqual(caught.exception.failure, "tooLarge")


if __name__ == "__main__":
    if sys.argv[1:] == ["--write"]:
        VECTORS.write_text(json.dumps(build_vectors(), indent=2, ensure_ascii=False) + "\n")
    else:
        unittest.main()
