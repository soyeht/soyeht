import contextlib
import io
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import release
from product_version import RELEASE_SOURCE, Release, Version

REAL = release.ROOT


def copy_tree(destination):
    """The files `release.py` reads or writes, copied from the repository."""
    paths = {place.path for place in release.PLACES} | {RELEASE_SOURCE}
    for pattern in release.MEMBER_MANIFESTS:
        paths |= {manifest.relative_to(REAL).as_posix() for manifest in REAL.glob(pattern)}
    for path in paths:
        (destination / path).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(REAL / path, destination / path)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        copy_tree(self.root)

    def edit(self, path, old, new, count=1):
        text = (self.root / path).read_text()
        self.assertIn(old, text)
        (self.root / path).write_text(text.replace(old, new, count))

    def test_the_repository_agrees_with_its_source(self):
        self.assertEqual(release.problems(REAL), [])

    def test_a_bump_rewrites_every_place_and_the_check_still_passes(self):
        old, new, changed = release.bump(self.root, "minor")
        self.assertEqual((old, new), (release.current(REAL), release.current(REAL).bumped("minor")))
        self.assertEqual(release.current(self.root), new)
        self.assertEqual(release.problems(self.root), [])
        for place in release.PLACES:
            if not place.optional:
                self.assertIn(place.path, changed)
        build = f'"CURRENT_PROJECT_VERSION" = "{new.build_number()}";'
        self.assertIn(build, (self.root / "apps/mac/SoyehtMac.xcodeproj/project.pbxproj").read_text())

    def test_a_bump_leaves_the_crates_from_the_registry_alone(self):
        before = (self.root / "engine/Cargo.lock").read_text()
        release.bump(self.root, "patch")
        after = (self.root / "engine/Cargo.lock").read_text()
        self.assertEqual(len(before.splitlines()), len(after.splitlines()))
        for old, new in zip(before.splitlines(), after.splitlines()):
            if old != new:
                self.assertEqual((old.startswith("version = "), new.startswith("version = ")), (True, True))
        blocks = [block for block in after.split("[[package]]\n") if "source = " in block]
        self.assertTrue(blocks)
        self.assertTrue(all(block in before for block in blocks))

    def test_an_explicit_version_must_go_up(self):
        old = release.current(self.root)
        higher = Release(Version(old.version.major + 1, 0, 0))
        self.assertEqual(release.bump(self.root, str(higher.version))[1], higher)
        for target in (str(higher.version), str(old.version), "0.0.0", "v999.0.0", "999.0", "999.0.0-rc1", "1000.0.0"):
            with self.assertRaises(ValueError, msg=target):
                release.bump(self.root, target)
        self.assertEqual(release.current(self.root), higher)

    def test_a_revision_uploads_the_same_version_again(self):
        old = release.current(self.root)
        _old, new, changed = release.bump(self.root, "revision")
        self.assertEqual((new.version, new.revision), (old.version, old.revision + 1))
        self.assertEqual(new.build_number(), old.build_number() + 1)
        self.assertEqual(release.problems(self.root), [])
        self.assertIn(RELEASE_SOURCE, changed)
        self.assertIn("apps/mac/SoyehtMac.xcodeproj/project.pbxproj", changed)
        self.assertIn("apps/ios/SoyehtPhone.xcodeproj/project.pbxproj", changed)
        # The version itself is not touched: no manifest, lockfile or package changes.
        self.assertEqual({path for path in changed if "Cargo" in path or "package" in path}, set())

    def test_a_new_version_resets_the_revision_to_zero(self):
        release.bump(self.root, "revision")
        release.bump(self.root, "revision")
        self.assertEqual(release.current(self.root).revision, 2)
        for target in ("patch", "minor", "major"):
            with self.subTest(target=target):
                _old, new, _changed = release.bump(self.root, target)
                self.assertEqual(new.revision, 0)
                self.assertEqual(release.problems(self.root), [])
                release.bump(self.root, "revision")
        release.bump(self.root, "revision")
        _old, new, _changed = release.bump(self.root, str(Version(new.version.major + 1, 0, 0)))
        self.assertEqual(new.revision, 0)

    def test_the_revision_stops_at_99(self):
        (self.root / RELEASE_SOURCE).write_text('{"productVersion": "0.1.0", "buildRevision": 99}')
        with self.assertRaises(ValueError):
            release.bump(self.root, "revision")
        self.assertEqual(release.current(self.root).revision, 99)

    def test_a_build_number_that_ignores_the_revision_is_found(self):
        release.bump(self.root, "revision")
        self.edit("apps/mac/SoyehtMac.xcodeproj/project.pbxproj", f'"CURRENT_PROJECT_VERSION" = "{release.current(self.root).build_number()}"', f'"CURRENT_PROJECT_VERSION" = "{release.current(self.root).build_number() - 1}"')
        found = release.problems(self.root)
        self.assertEqual(len(found), 1, found)
        self.assertIn("CURRENT_PROJECT_VERSION", found[0])

    def test_a_dry_run_writes_nothing(self):
        before = {place.path: (self.root / place.path).read_text() for place in release.PLACES}
        _old, _new, changed = release.bump(self.root, "major", dry_run=True)
        self.assertIn("engine/Cargo.toml", changed)
        self.assertEqual(before, {place.path: (self.root / place.path).read_text() for place in release.PLACES})
        self.assertEqual(release.problems(self.root), [])

    def test_a_bump_that_cannot_find_a_place_writes_nothing(self):
        (self.root / "apps/desktop/Cargo.toml").write_text("[workspace.package]\nedition = \"2024\"\n")
        source = (self.root / RELEASE_SOURCE).read_text()
        with self.assertRaises(ValueError) as failure:
            release.bump(self.root, "patch")
        self.assertIn("apps/desktop/Cargo.toml", str(failure.exception))
        self.assertEqual((self.root / RELEASE_SOURCE).read_text(), source)
        self.assertIn(f'version = "{release.current(self.root).version}"', (self.root / "engine/Cargo.toml").read_text())

    def test_a_place_that_is_out_of_step_is_named(self):
        made = release.current(REAL)
        version, build = made.version, made.build_number()
        cases = {
            "engine/Cargo.toml": (f'version = "{version}"', 'version = "0.0.0"'),
            "apps/desktop/Cargo.toml": (f'version = "{version}"', 'version = "0.0.0"'),
            "contracts/typescript/package.json": (f'"version": "{version}"', '"version": "0.0.0"'),
            "apps/ios/SoyehtPhone.xcodeproj/project.pbxproj": (f'"MARKETING_VERSION" = "{version}"', '"MARKETING_VERSION" = "0.0.0"'),
            "apps/mac/SoyehtMac.xcodeproj/project.pbxproj": (f'"CURRENT_PROJECT_VERSION" = "{build}"', '"CURRENT_PROJECT_VERSION" = "0"'),
        }
        for path, (old, new) in cases.items():
            with self.subTest(path=path):
                self.edit(path, old, new)
                found = release.problems(self.root)
                self.assertEqual(len(found), 1, found)
                self.assertTrue(found[0].startswith(path), found)
                self.edit(path, new, old)
                self.assertEqual(release.problems(self.root), [])

    def test_a_lockfile_entry_of_a_workspace_crate_is_checked(self):
        version = release.current(REAL).version
        self.edit("apps/desktop/Cargo.lock", f'name = "soyeht-client"\nversion = "{version}"', 'name = "soyeht-client"\nversion = "0.0.0"')
        found = release.problems(self.root)
        self.assertEqual(len(found), 1, found)
        self.assertIn("soyeht-client", found[0])

    def test_a_crate_with_a_version_of_its_own_is_found(self):
        manifest = self.root / "engine/soyeht-store/Cargo.toml"
        manifest.write_text(manifest.read_text().replace("version.workspace = true", 'version = "0.0.0"'))
        found = release.problems(self.root)
        self.assertEqual(len(found), 1, found)
        self.assertIn("engine/soyeht-store/Cargo.toml", found[0])

    def test_a_literal_version_in_a_plist_is_found_and_put_back_to_the_setting(self):
        self.edit("apps/ios/Info.plist", "<string>$(MARKETING_VERSION)</string>", "<string>9.9.9</string>")
        found = release.problems(self.root)
        self.assertEqual(len(found), 1, found)
        self.assertIn("CFBundleShortVersionString", found[0])
        release.apply(self.root, release.current(self.root))
        self.assertEqual(release.problems(self.root), [])

    def test_a_missing_source_or_file_is_a_problem_not_a_crash(self):
        (self.root / "engine/Cargo.lock").unlink()
        self.assertTrue(any("engine/Cargo.lock" in line for line in release.problems(self.root)))
        (self.root / RELEASE_SOURCE).write_text('{"productVersion": "one"}')
        self.assertEqual(len(release.problems(self.root)), 1)


class CommandTests(unittest.TestCase):
    def run_main(self, *arguments):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            status = release.main(list(arguments))
        return status, out.getvalue(), err.getvalue()

    def test_show_prints_the_version_the_build_number_and_the_source(self):
        made = release.current(REAL)
        status, out, _ = self.run_main("show")
        self.assertEqual(status, 0)
        self.assertIn(str(made.version), out)
        self.assertIn(str(made.build_number()), out)
        self.assertIn(RELEASE_SOURCE, out)
        self.assertEqual(self.run_main("show", "version")[1], f"{made.version}\n")
        self.assertEqual(self.run_main("show", "revision")[1], f"{made.revision}\n")
        self.assertEqual(self.run_main("show", "build-number")[1], f"{made.build_number()}\n")

    def test_check_fails_and_names_the_place_when_one_disagrees(self):
        with patch.object(release, "problems", return_value=["engine/Cargo.toml:6 is 0.2.0"]):
            status, _, err = self.run_main("check")
        self.assertEqual(status, 1)
        self.assertIn("engine/Cargo.toml:6", err)

    def test_check_also_runs_the_generator_check(self):
        with patch.object(release, "run_generator", return_value=1) as generator:
            status, _, _ = self.run_main("check")
        self.assertEqual(status, 1)
        generator.assert_called_once_with("--check")

    def test_bump_regenerates_after_writing(self):
        with patch.object(release, "bump", return_value=(Release(Version(0, 1, 0)), Release(Version(0, 1, 1)), [])), \
                patch.object(release, "run_generator", return_value=0) as generator:
            status, out, _ = self.run_main("bump", "patch")
        self.assertEqual(status, 0)
        generator.assert_called_once_with()
        self.assertIn("0.1.0 revision 0 (build number 100000) -> 0.1.1 revision 0 (build number 100100)", out)

    def test_a_generator_that_fails_after_the_write_names_the_fix(self):
        with patch.object(release, "bump", return_value=(Release(Version(0, 1, 0)), Release(Version(0, 1, 1)), [])), \
                patch.object(release, "run_generator", return_value=1):
            status, _, err = self.run_main("bump", "patch")
        self.assertEqual(status, 1)
        self.assertIn("scripts/release.py sync", err)

    def test_a_refused_bump_exits_with_the_reason_and_regenerates_nothing(self):
        with patch.object(release, "run_generator") as generator:
            status, _, err = self.run_main("bump", "0.0.1")
        self.assertEqual(status, 1)
        self.assertIn("only goes up", err)
        generator.assert_not_called()

    def test_there_is_no_command_that_pretends_to_package_or_notarize(self):
        self.assertEqual(set(release.COMMANDS), {"show", "bump", "sync", "check"})


if __name__ == "__main__":
    unittest.main()
