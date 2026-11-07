"""Regression tests for the crate layer check."""

from __future__ import annotations

import tempfile
import tomllib
import unittest
from pathlib import Path

from check_layers import (
    LAYERS,
    ROOT,
    WORKSPACE_ROOTS,
    manifest_problems,
    problems,
    soyeht_dependencies,
    workspace_problems,
)


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

    def test_a_crate_may_enable_its_own_feature_for_its_tests(self):
        own = manifest("soyeht-protocol", '[dev-dependencies]\nsoyeht-protocol = { path = ".", features = ["x"] }\n')
        self.assertEqual(manifest_problems("p/Cargo.toml", own), [])

    def test_a_lower_layer_cannot_depend_on_the_engine(self):
        upward = manifest("soyeht-protocol", '[dependencies]\nsoyeht-engine = { path = "../e" }\n')
        self.assertEqual(len(manifest_problems("p/Cargo.toml", upward)), 1)

    def test_a_crate_may_list_itself_to_turn_on_its_own_features_for_tests(self):
        own = manifest(
            "soyeht-engine",
            '[dev-dependencies]\nsoyeht-engine = { path = ".", features = ["test-hooks"] }\n',
        )
        self.assertEqual(manifest_problems("e/Cargo.toml", own), [])

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

    def test_only_the_command_line_registers_the_service(self):
        body = '[dependencies]\nsoyeht-service = { path = "../s" }\n'
        self.assertEqual(manifest_problems("c/Cargo.toml", manifest("soyeht-cli", body)), [])
        self.assertEqual(len(manifest_problems("e/Cargo.toml", manifest("soyeht-engine", body))), 1)
        upward = '[dependencies]\nsoyeht-store = { path = "../s" }\n'
        self.assertEqual(len(manifest_problems("s/Cargo.toml", manifest("soyeht-service", upward))), 1)

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


VT100 = '{ git = "https://github.com/soyeht/vt100-rust", rev = "aaaa" }'
WORKSPACE = f"""[workspace]
members = []
[workspace.package]
rust-version = "1.98"
[workspace.lints.rust]
unsafe_op_in_unsafe_fn = "deny"
[workspace.lints.clippy]
panic = "deny"
[patch.crates-io]
vt100 = {VT100}
"""


class WorkspaceTests(unittest.TestCase):
    def workspaces(self, **replacements: tuple[str, str]) -> list[str]:
        """Write the three workspace manifests, with `base: (old, new)` edits, and check them."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for base in WORKSPACE_ROOTS:
                text = WORKSPACE
                if base in replacements:
                    old, new = replacements[base]
                    self.assertIn(old, text)
                    text = text.replace(old, new)
                (root / base).mkdir(parents=True)
                (root / base / "Cargo.toml").write_text(text)
            return workspace_problems(root)

    def test_the_repository_workspaces_match(self):
        self.assertEqual(workspace_problems(ROOT), [])

    def test_equal_workspaces_pass(self):
        self.assertEqual(self.workspaces(), [])

    def test_a_different_vt100_revision_fails(self):
        found = self.workspaces(**{"apps/android": ('rev = "aaaa"', 'rev = "bbbb"')})
        self.assertEqual(len(found), 1)
        self.assertIn("apps/android/Cargo.toml: [patch.crates-io] vt100", found[0])

    def test_a_missing_vt100_patch_fails(self):
        found = self.workspaces(**{"apps/desktop": (f"vt100 = {VT100}", "")})
        self.assertEqual(len(found), 1)
        self.assertIn("apps/desktop/Cargo.toml: [patch.crates-io] vt100 is missing", found[0])

    def test_another_patched_crate_is_not_compared(self):
        found = self.workspaces(**{"apps/desktop": (f"vt100 = {VT100}", f'vt100 = {VT100}\ncalloop = "1"')})
        self.assertEqual(found, [])

    def test_a_different_rust_version_fails(self):
        found = self.workspaces(engine=('rust-version = "1.98"', 'rust-version = "1.99"'))
        self.assertEqual(len(found), 2)
        self.assertTrue(all("rust-version" in line for line in found))

    def test_a_different_lint_table_fails(self):
        found = self.workspaces(**{"apps/desktop": ('panic = "deny"', 'panic = "warn"')})
        self.assertEqual(len(found), 1)
        self.assertIn("apps/desktop/Cargo.toml: [workspace.lints]", found[0])

    def test_a_workspace_without_a_manifest_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for base in WORKSPACE_ROOTS[:2]:
                (root / base).mkdir(parents=True)
                (root / base / "Cargo.toml").write_text(WORKSPACE)
            found = workspace_problems(root)
        self.assertEqual(found, ["apps/android/Cargo.toml: no workspace manifest to compare"])


if __name__ == "__main__":
    unittest.main()
