import copy
import json
import unittest
from pathlib import Path

import check_legacy_isolation as check
import legacy_guard

ROOT = Path(__file__).resolve().parents[1]


class ProfileTests(unittest.TestCase):
    def setUp(self):
        self.document = json.loads((ROOT / check.PROFILES).read_text())

    def test_the_real_profiles_belong_to_the_next_family(self):
        self.assertEqual(check.profile_problems(self.document), [])

    def test_a_value_without_next_or_with_a_legacy_name_fails(self):
        for path, value in (
            (("dev", "dataDirectory", "macos"), "Soyeht/dev"),
            (("prod", "dataDirectory", "linux"), "soyeht"),
            (("prod", "appIds", "macos"), "com.soyeht.mac"),
            (("dev", "appIds", "macos"), "com.soyeht.mac.dev"),
            (("prod", "launchdLabels", "engine"), "com.soyeht.engine"),
        ):
            changed = copy.deepcopy(self.document)
            target = changed["profiles"]
            for key in path[:-1]:
                target = target[key]
            target[path[-1]] = value
            with self.subTest(path=path):
                self.assertEqual(len(check.profile_problems(changed)), 1)


class FileTests(unittest.TestCase):
    def test_every_legacy_name_is_found_outside_the_allowed_files(self):
        for text in (
            "open /Applications/Soyeht.app",
            'bundle = "com.soyeht.mac"',
            "id com.soyeht.mac.dev",
            "rm ~/Library/Application Support/Soyeht",
            "rm ~/Library/Application Support/Soyeht Dev/x",
            "open /applications/soyeht.app",
            "id com.Soyeht.mac.engine",
            "rm ~/Library/Application Support/SOYEHT",
        ):
            with self.subTest(text=text):
                self.assertEqual(len(check.file_problems({"scripts/x.py": text})), 1)

    def test_names_of_the_rewrite_and_the_allowed_files_pass(self):
        texts = {
            "docs/a.md": "~/Library/Application Support/Soyeht.next/dev and com.soyeht.next.mac",
            "docs/b.md": "Application Support/Soyeht-next and com.soyeht.macro",
            "AGENTS.md": "/Applications/Soyeht.app",
        }
        self.assertEqual(check.file_problems(texts), [])

    def test_the_tracked_repository_names_no_legacy_app(self):
        self.assertEqual(check.file_problems(check.tracked_texts(ROOT)), [])


class GuardTests(unittest.TestCase):
    def test_values_without_next_are_refused(self):
        with self.assertRaises(SystemExit):
            legacy_guard.require_next("label", "com.soyeht.engine")
        with self.assertRaises(SystemExit):
            legacy_guard.require_next("label", "com.Soyeht.mac.engine next")
        self.assertEqual(legacy_guard.require_next("label", "com.soyeht.next.engine"), "com.soyeht.next.engine")

    def test_the_legacy_location_is_refused_even_when_it_names_next(self):
        for path in (
            "/Applications/Soyeht.app/next",
            "/Users/a/Library/Application Support/Soyeht/next",
            "/Users/a/Library/Application Support/Soyeht Dev/next",
            "/applications/soyeht.app/next",
            "/Users/a/Library/Application Support/SOYEHT/next",
            "/Users/a/library/application support/soyeht dev/next",
        ):
            with self.subTest(path=path), self.assertRaises(SystemExit):
                legacy_guard.require_next_path("path", Path(path))
        legacy_guard.require_next_path("path", Path("/Users/a/Library/Application Support/Soyeht.next/dev"))


if __name__ == "__main__":
    unittest.main()
