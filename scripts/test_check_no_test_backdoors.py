"""Regression tests for the test back door check."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from check_no_test_backdoors import (
    ALLOWED,
    ENV_NAMES_FROM_LISTS,
    allowlist_problems,
    manifest_problems,
    problems,
    source_problems,
)

PRODUCTION = '''
#[cfg(debug_assertions)]
fn pause(directory: &Path) -> bool {
    directory.join("test-pause-autosave").is_file()
}

fn poll() -> bool {
    std::env::var_os("SOYEHT_TEST_SKILL_POLL").is_some() || cfg!(debug_assertions)
}

#[cfg(feature = "test-hooks")]
mod hooks;

// debug_assertions and "test-in-a-comment" are prose.
fn quiet() {
    debug_assert!(true);
    let name = "testing-room";
}

#[cfg(test)]
mod tests {
    fn marker() -> &'static str {
        "test-code"
    }
}
'''


class SourceTests(unittest.TestCase):
    def test_reports_each_back_door_in_production_code(self):
        found = source_problems("engine/soyeht-engine/src/a.rs", PRODUCTION, set())
        self.assertEqual(
            found,
            [
                (2, "branches on debug_assertions"),
                (4, 'test marker "test-pause-autosave"'),
                (8, 'test marker "SOYEHT_TEST_SKILL_POLL"'),
                (8, "branches on debug_assertions"),
            ],
        )

    def test_a_test_only_file_is_exempt(self):
        path = "engine/soyeht-engine/src/a_tests.rs"
        self.assertEqual(source_problems(path, PRODUCTION, {path}), [])

    def test_an_allowed_file_is_exempt_and_every_allowance_has_a_reason(self):
        path = next(iter(ALLOWED))
        self.assertEqual(source_problems(path, PRODUCTION, set()), [])
        self.assertTrue(all(reason.strip() for reason in ALLOWED.values()))


ENVIRONMENT = '''
const MODE: &str = "SOYEHT_MODE";

fn read(name: &str) {
    let literal = std::env::var("PATH");
    let constant = env::var_os(MODE);
    let qualified = std::env::var(crate::names::HELPER_MODE);
    let built = std::env::var(format!("SOYEHT_{name}"));
    let listed = std::env::var_os(name);
    let everything = std::env::vars_os();
}

#[cfg(test)]
mod tests {
    fn helper(name: &str) {
        let _ = std::env::var(name);
    }
}
'''


class EnvironmentTests(unittest.TestCase):
    def test_a_name_built_at_run_time_is_reported_and_a_literal_or_constant_is_not(self):
        found = source_problems("engine/soyeht-demo/src/a.rs", ENVIRONMENT, set())
        self.assertEqual(
            [(line, message.split("::", 1)[1]) for line, message in found],
            [
                (8, 'var(format!("SOYEHT_{name}"))'),
                (9, "var_os(name)"),
                (10, "vars_os()"),
            ],
        )

    def test_a_listed_read_passes_and_is_marked_used(self):
        key = next(iter(ENV_NAMES_FROM_LISTS))
        path, call = key.split("::", 1)
        function, argument = call[:-1].split("(", 1)
        text = f"fn f(name: &str) {{ let _ = std::env::{function}({argument}); }}\n"
        used: set[str] = set()
        self.assertEqual(source_problems(path, text, set(), used), [])
        self.assertEqual(used, {key})

    def test_every_listed_read_has_a_reason(self):
        self.assertTrue(all(reason.strip() for reason in ENV_NAMES_FROM_LISTS.values()))


class MarkerTests(unittest.TestCase):
    def test_the_marker_covers_underscore_and_dot_prefixes(self):
        text = (
            'fn f() { a("test_flag"); b(".test-worktrees"); c("testing"); '
            'd("latest-test-x"); e("test-x"); }\n'
        )
        found = source_problems("engine/soyeht-demo/src/a.rs", text, set())
        self.assertEqual(
            [message for _, message in found],
            ['test marker "test_flag"', 'test marker ".test-worktrees"', 'test marker "test-x"'],
        )


class ManifestTests(unittest.TestCase):
    def test_a_default_feature_ships_the_hooks(self):
        manifest = {"features": {"default": ["test-hooks"], "test-hooks": []}}
        self.assertEqual(len(manifest_problems("engine/x/Cargo.toml", manifest)), 1)

    def test_a_normal_dependency_may_not_enable_them_but_a_dev_dependency_may(self):
        shipped = {"dependencies": {"soyeht-engine": {"path": "..", "features": ["test-hooks"]}}}
        self.assertEqual(len(manifest_problems("engine/x/Cargo.toml", shipped)), 1)
        target = {"target": {"cfg(unix)": shipped}}
        self.assertEqual(len(manifest_problems("engine/x/Cargo.toml", target)), 1)
        tests_only = {"dev-dependencies": {"soyeht-engine": {"features": ["test-hooks"]}}}
        self.assertEqual(manifest_problems("engine/x/Cargo.toml", tests_only), [])


class AllowlistTests(unittest.TestCase):
    def tree(self, directory: str, parent: str | None) -> Path:
        root = Path(directory)
        for relative in ALLOWED:
            file = root / relative
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text("// hooks\n")
            if parent is not None:
                name = file.stem
                module = file.parent / ("lib.rs" if file.parent.name == "src" else "mod.rs")
                module.write_text(parent.format(name=name))
        return root

    def test_a_gated_module_passes(self):
        gated = '#[cfg(feature = "test-hooks")]\npub mod {name};\n'
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(allowlist_problems(self.tree(directory, gated)), [])

    def test_a_module_declared_without_the_feature_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            found = allowlist_problems(self.tree(directory, "pub mod {name};\n"))
        self.assertEqual(len(found), len(ALLOWED))
        self.assertTrue(all("not declared under" in line for line in found))

    def test_a_missing_file_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            found = allowlist_problems(Path(directory))
        self.assertEqual(len(found), len(ALLOWED))
        self.assertTrue(all("missing" in line for line in found))


class RepositoryTests(unittest.TestCase):
    def test_the_repository_has_no_back_doors(self):
        self.assertEqual(problems(Path(__file__).resolve().parents[1]), [])

    def test_a_crate_with_a_marker_is_reported(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "engine/soyeht-demo/src"
            source.mkdir(parents=True)
            (source / "lib.rs").write_text('fn f() -> bool { Path::new("test-skip").exists() }\n')
            (root / "engine/soyeht-demo/Cargo.toml").write_text(
                '[features]\ndefault = ["test-hooks"]\ntest-hooks = []\n'
            )
            found = problems(root)
        self.assertIn('engine/soyeht-demo/src/lib.rs:1: test marker "test-skip"', found)
        self.assertTrue(any("default feature" in line for line in found))
        # Nothing of the real allowlists exists in this tree, so every entry is stale.
        self.assertEqual(sum("no longer exists" in line for line in found), len(ENV_NAMES_FROM_LISTS))
        self.assertEqual(sum("allowlisted but missing" in line for line in found), len(ALLOWED))


if __name__ == "__main__":
    unittest.main()
