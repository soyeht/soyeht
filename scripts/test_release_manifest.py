import base64
import contextlib
import hashlib
import io
import json
import tempfile
import unittest
from pathlib import Path

import release_manifest  # first: it puts the contracts directory on the path
import ed25519  # noqa: E402
import update_manifest as manifest  # noqa: E402

# A throwaway key that signs nothing but these tests.
SEED = hashlib.sha256(b"soyeht release manifest script test key").digest()
KEYS = [{"id": "test-key", "key": base64.b64encode(ed25519.public_key(SEED)).decode()}]
ENVIRONMENT = {release_manifest.KEY_ENVIRONMENT: base64.b64encode(SEED).decode()}
VERSION = release_manifest.product_version.load(
    (release_manifest.ROOT / "contracts/release.json").read_text()
).version


class ReleaseManifestTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.dist = Path(directory.name)

    def package(self, name, content=b"package"):
        (self.dist / name).write_bytes(content)

    def packages(self):
        self.package(f"soyeht-{VERSION}-macos-aarch64.dmg", b"mac")
        self.package(f"soyeht-{VERSION}-linux-x86_64.tar.gz", b"linux")
        self.package(f"soyeht-{VERSION}-windows-x86_64.zip", b"windows")
        (self.dist / "notes.txt").write_text("not a package")

    def test_a_manifest_lists_each_package_with_its_hash_and_size_and_a_client_accepts_it(self):
        self.packages()
        text, packages = release_manifest.build(self.dist, ENVIRONMENT, "0.0.0", KEYS)
        self.assertEqual({(p["system"], p["architecture"]) for p in packages},
                         {("macos", "aarch64"), ("linux", "x86_64"), ("windows", "x86_64")})
        linux = next(p for p in packages if p["system"] == "linux")
        self.assertEqual(linux["sha256"], hashlib.sha256(b"linux").hexdigest())
        self.assertEqual(linux["size"], 5)
        self.assertTrue(linux["url"].endswith(f"/v{VERSION}/soyeht-{VERSION}-linux-x86_64.tar.gz"))
        older = (0, 0, 1)
        offer = manifest.evaluate(text, older, "linux", "x86_64", "stable", KEYS)
        self.assertEqual(offer["version"], str(VERSION))
        self.assertEqual(offer["package"]["sha256"], linux["sha256"])
        self.assertEqual(manifest.evaluate(text, tuple(VERSION), "linux", "x86_64", "stable", KEYS), {"status": "upToDate"})

    def test_a_tampered_manifest_is_refused_by_verify(self):
        self.packages()
        text, _ = release_manifest.build(self.dist, ENVIRONMENT, "0.0.0", KEYS)
        document = json.loads(text)
        payload = json.loads(base64.b64decode(document["payload"]))
        payload["packages"][0]["url"] = "https://github.com/soyeht/soyeht/releases/download/evil/other.dmg"
        document["payload"] = base64.b64encode(json.dumps(payload).encode()).decode()
        with self.assertRaises(manifest.Failed) as caught:
            release_manifest.describe(json.dumps(document), KEYS, None, "macos", "aarch64", "stable")
        self.assertEqual(caught.exception.failure, "badSignature")

    def test_an_architecture_alias_is_written_in_the_rust_spelling(self):
        self.package(f"soyeht-{VERSION}-macos-arm64.dmg")
        _, packages = release_manifest.build(self.dist, ENVIRONMENT, "0.0.0", KEYS)
        self.assertEqual(packages[0]["architecture"], "aarch64")

    def test_the_signing_key_must_be_given_and_be_a_32_byte_seed(self):
        self.packages()
        for environment in ({}, {release_manifest.KEY_ENVIRONMENT: "not base64!"},
                            {release_manifest.KEY_ENVIRONMENT: base64.b64encode(b"short").decode()}):
            with self.assertRaises(release_manifest.Refused):
                release_manifest.build(self.dist, environment, "0.0.0", KEYS)

    def test_a_key_the_contract_does_not_carry_is_refused_with_the_line_to_add(self):
        self.packages()
        with self.assertRaises(release_manifest.Refused) as caught:
            release_manifest.build(self.dist, ENVIRONMENT, "0.0.0", [])
        self.assertIn(KEYS[0]["key"], str(caught.exception))

    def test_a_package_of_another_version_or_a_repeated_target_stops_the_build(self):
        self.package("soyeht-9.9.9-macos-aarch64.dmg")
        with self.assertRaises(release_manifest.Refused):
            release_manifest.build(self.dist, ENVIRONMENT, "0.0.0", KEYS)
        (self.dist / "soyeht-9.9.9-macos-aarch64.dmg").unlink()
        self.package(f"soyeht-{VERSION}-macos-aarch64.dmg")
        self.package(f"soyeht-{VERSION}-macos-arm64.zip")
        with self.assertRaises(release_manifest.Refused):
            release_manifest.build(self.dist, ENVIRONMENT, "0.0.0", KEYS)

    def test_an_empty_dist_and_an_unknown_system_are_refused(self):
        with self.assertRaises(release_manifest.Refused):
            release_manifest.build(self.dist, ENVIRONMENT, "0.0.0", KEYS)
        self.package(f"soyeht-{VERSION}-solaris-x86_64.pkg")
        with self.assertRaises(release_manifest.Refused):
            release_manifest.build(self.dist, ENVIRONMENT, "0.0.0", KEYS)

    def test_a_minimum_above_the_version_breaks_the_format(self):
        self.packages()
        with self.assertRaises(release_manifest.Refused):
            release_manifest.build(self.dist, ENVIRONMENT, "999.0.0", KEYS)

    def test_keygen_prints_a_seed_the_build_accepts_and_the_line_for_the_contract(self):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            release_manifest.main(["keygen"])
        seed = out.getvalue().strip()
        public = json.loads(err.getvalue().strip().splitlines()[-1])["key"]
        self.assertEqual(base64.b64encode(ed25519.public_key(base64.b64decode(seed))).decode(), public)
        self.assertEqual(len(base64.b64decode(seed)), 32)

    def test_main_writes_the_manifest_and_verify_reads_it_and_refuses_a_stranger(self):
        self.packages()
        output = self.dist / "update-manifest.json"
        keys = release_manifest.POLICY["signature"]["publicKeys"]
        kept = list(keys)
        keys[:] = KEYS
        self.addCleanup(lambda: keys.__setitem__(slice(None), kept))
        with contextlib.redirect_stdout(io.StringIO()):
            release_manifest.main(["build", "--dist", str(self.dist)], ENVIRONMENT)
        shown = io.StringIO()
        with contextlib.redirect_stdout(shown):
            release_manifest.main(["verify", str(output), "--running", "0.0.1", "--system", "windows",
                                   "--architecture", "x86_64"])
        self.assertIn("windows-x86_64", shown.getvalue())
        self.assertIn('"status": "available"', shown.getvalue())
        keys[:] = [{"id": "test-key", "key": base64.b64encode(ed25519.public_key(b"\x07" * 32)).decode()}]
        with self.assertRaises(SystemExit) as refused:
            release_manifest.main(["verify", str(output)])
        self.assertIn("badSignature", str(refused.exception))


if __name__ == "__main__":
    unittest.main()
