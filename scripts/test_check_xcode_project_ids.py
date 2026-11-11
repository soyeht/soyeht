"""Regression tests for the Xcode project id check."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from check_xcode_project_ids import problems

BUILD = "AAAAAAAAAAAAAAAAAAAAAAAA"
FILE = "BBBBBBBBBBBBBBBBBBBBBBBB"
MISSING = "CCCCCCCCCCCCCCCCCCCCCCCC"

PROJECT = """
// !$*UTF8*$!
{
	"objectVersion" = "60";
	"objects" = {
		"AAAAAAAAAAAAAAAAAAAAAAAA" = {
			"isa" = "PBXBuildFile";
			"fileRef" = "BBBBBBBBBBBBBBBBBBBBBBBB";
		};
		"BBBBBBBBBBBBBBBBBBBBBBBB" = {
			"isa" = "PBXFileReference";
			"path" = "A.swift";
		};
	};
	"rootObject" = "BBBBBBBBBBBBBBBBBBBBBBBB";
}
"""


class XcodeProjectIdTests(unittest.TestCase):
    def check(self, text: str) -> list[str]:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "apps/mac/App.xcodeproj/project.pbxproj"
            path.parent.mkdir(parents=True)
            path.write_text(text, encoding="utf-8")
            return problems(Path(directory))

    def test_accepts_unique_ids_and_resolved_file_references(self) -> None:
        self.assertEqual(self.check(PROJECT), [])

    def test_accepts_an_unquoted_key_with_a_comment(self) -> None:
        text = PROJECT.replace(
            f'\t\t"{FILE}" = {{', f"\t\t{FILE} /* A.swift */ = {{"
        )
        self.assertEqual(self.check(text), [])

    def test_rejects_an_object_id_defined_twice(self) -> None:
        duplicate = (
            f'\t\t"{FILE}" = {{\n'
            '\t\t\t"isa" = "PBXFileReference";\n'
            '\t\t\t"path" = "B.swift";\n'
            "\t\t};\n"
        )
        text = PROJECT.replace('\t};\n\t"rootObject"', duplicate + '\t};\n\t"rootObject"')
        found = self.check(text)
        self.assertEqual(len(found), 1)
        self.assertIn(FILE, found[0])
        self.assertIn("lines 10, 14", found[0])

    def test_rejects_a_file_reference_that_is_not_an_object(self) -> None:
        text = PROJECT.replace(f'"fileRef" = "{FILE}"', f'"fileRef" = "{MISSING}"')
        found = self.check(text)
        self.assertEqual(len(found), 1)
        self.assertIn(BUILD, found[0])
        self.assertIn(MISSING, found[0])
        self.assertIn("which is not an object", found[0])


if __name__ == "__main__":
    unittest.main()
