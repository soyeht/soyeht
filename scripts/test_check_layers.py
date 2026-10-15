"""Regression tests for the crate layer check."""

from __future__ import annotations

import tempfile
import tomllib
import unittest
from pathlib import Path

from check_layers import LAYERS, ROOT, manifest_problems, problems, soyeht_dependencies


def manifest(name: str, body: str) -> dict:
    return tomllib.loads(f'[package]\nname = "{name}"\n{body}')


class LayerTests(unittest.TestCase):
    def test_the_repository_follows_the_layers(self):
        self.assertEqual(problems(ROOT), [])

    def test_a_block_may_use_the_foundation_but_not_another_block(self):
        allowed = manifest("soyeht-editor", '[dependencies]\nsoyeht-protocol = { path = "../p" }\n')
        self.assertEqual(manifest_problems("e/Cargo.toml", allowed), [])
        sideways = manifest("soyeht-editor", '[dependencies]\nsoyeht-store = { path = "../s" }\n')
        found = manifest_problems("e/Cargo.toml", sideways)
        self.assertEqual(len(found), 1)
        self.assertIn("soyeht-editor depends on soyeht-store", found[0])

    def test_a_lower_layer_cannot_depend_on_the_engine(self):
        upward = manifest("soyeht-protocol", '[dependencies]\nsoyeht-engine = { path = "../e" }\n')
        self.assertEqual(len(manifest_problems("p/Cargo.toml", upward)), 1)

    def test_dev_build_and_target_tables_are_checked(self):
        body = (
            '[dev-dependencies]\nsoyeht-engine = { path = "../e" }\n'
            '[build-dependencies]\nsoyeht-store = { path = "../s" }\n'
            "[target.'cfg(windows)'.dependencies]\nsoyeht-acp = { path = \"../a\" }\n"
        )
        found = manifest_problems("v/Cargo.toml", manifest("soyeht-vcs", body))
        self.assertEqual(len(found), 3)

    def test_a_renamed_dependency_is_seen_by_its_package_name(self):
        renamed = manifest("soyeht-vcs", '[dependencies]\nlooks_harmless = { package = "soyeht-engine", path = "../e" }\n')
        self.assertEqual(soyeht_dependencies(renamed), {"soyeht-engine"})

    def test_external_crates_are_not_limited(self):
        external = manifest("soyeht-protocol", '[dependencies]\nserde = "1"\n')
        self.assertEqual(manifest_problems("p/Cargo.toml", external), [])

    def test_the_desktop_client_stays_on_protocol_and_platform(self):
        engine = manifest("soyeht-client", '[dependencies]\nsoyeht-store = { path = "../s" }\n')
        self.assertEqual(len(manifest_problems("c/Cargo.toml", engine)), 1)

    def test_a_crate_in_no_layer_fails(self):
        found = manifest_problems("n/Cargo.toml", manifest("soyeht-new", ""))
        self.assertIn("is in no layer", found[0])

    def test_a_workspace_manifest_is_skipped(self):
        self.assertEqual(manifest_problems("Cargo.toml", {"workspace": {"members": []}}), [])

    def test_a_layer_entry_without_a_manifest_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "engine").mkdir()
            (root / "apps/desktop").mkdir(parents=True)
            found = problems(root)
        self.assertEqual(len(found), len(LAYERS))
        self.assertTrue(all("has no Cargo.toml" in line for line in found))


if __name__ == "__main__":
    unittest.main()
