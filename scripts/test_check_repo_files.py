"""Regression tests for the repository file check."""

from __future__ import annotations

import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import check_repo_files
from check_repo_files import MAX_BYTES, history_problems, problems_for, tree_problems


def git(root: Path, *args: str) -> str:
    return subprocess.run(
        ["git", "-c", "user.name=t", "-c", "user.email=t@t", *args],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()


class ProblemsForTests(unittest.TestCase):
    def test_text_source_passes(self):
        self.assertEqual(problems_for("engine/a/src/lib.rs", b"fn main() {}\n", 13), [])

    def test_archives_and_native_files_are_rejected_by_name(self):
        for name in ("a.tgz", "b.zip", "c.tar", "d.dylib", "e.exe", "f/g.log", "h.orig", ".DS_Store"):
            self.assertTrue(problems_for(name, b"text", 4), name)

    def test_a_renamed_archive_or_executable_is_rejected_by_content(self):
        cases = {
            b"\x1f\x8b\x08\x00": "gzip",
            b"PK\x03\x04rest": "zip",
            b"\x7fELF\x02": "ELF",
            b"\xcf\xfa\xed\xfe\x07": "Mach-O",
            b"MZ\x90\x00\x03\x00\x00\x00": "Windows",
        }
        for head, kind in cases.items():
            found = problems_for("tools/helper", head, len(head))
            self.assertTrue(any(kind in line for line in found), (kind, found))

    def test_a_text_file_that_starts_with_mz_is_not_an_executable(self):
        self.assertEqual(problems_for("docs/a.md", b"MZ is a code", 12), [])

    def test_binaries_are_allowed_only_in_their_directories(self):
        png = b"\x89PNG\r\n\x1a\n\x00\x00"
        self.assertEqual(problems_for("docs/reports/slice-1/a.png", png, 10), [])
        self.assertEqual(problems_for("apps/mac/Resources/Fonts/A.ttf", b"\x00\x01\x00\x00", 4), [])
        self.assertTrue(problems_for("scripts/a.png", png, 10))
        icon = b"\x00\x00\x01\x00\x01\x00"
        self.assertEqual(problems_for("apps/desktop/packaging/windows/soyeht.ico", icon, 6), [])
        self.assertEqual(
            problems_for("apps/desktop/packaging/linux/icons/hicolor/16x16/apps/soyeht.png", png, 10),
            [],
        )
        self.assertTrue(problems_for("scripts/soyeht.ico", icon, 6))
        self.assertTrue(problems_for("docs/notes.mp4", b"\x00\x00\x00\x18ftyp", 8))
        self.assertTrue(problems_for("data/blob", b"\x00\x01\x02", 3))

    def test_an_allowed_file_skips_the_type_rules_but_not_the_size_cap(self):
        with patch.dict(check_repo_files.ALLOWED_FILES, {"tools/clip.mp4": "demo"}):
            self.assertEqual(problems_for("tools/clip.mp4", b"\x00\x00ftyp", 6), [])
            self.assertTrue(problems_for("tools/clip.mp4", b"\x00\x00ftyp", MAX_BYTES + 1))

    def test_size_cap(self):
        self.assertEqual(problems_for("docs/a.md", b"x", MAX_BYTES), [])
        self.assertTrue(problems_for("docs/a.md", b"x", MAX_BYTES + 1))


class RepositoryTests(unittest.TestCase):
    def repository(self) -> Path:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        git(root, "init", "-q")
        (root / "README.md").write_text("hello\n")
        git(root, "add", "-A")
        git(root, "commit", "-qm", "base")
        return root

    def commit(self, root: Path, files: dict[str, bytes], message: str) -> None:
        for name, content in files.items():
            (root / name).parent.mkdir(parents=True, exist_ok=True)
            (root / name).write_bytes(content)
        git(root, "add", "-A")
        git(root, "commit", "-qm", message)

    def test_tree_check_reports_tracked_archives(self):
        root = self.repository()
        self.commit(root, {"dist/bundle.tgz": b"\x1f\x8b\x08\x00data"}, "add")
        with patch.dict(check_repo_files.ALLOWED_FILES, {}, clear=True):
            found = tree_problems(root)
        self.assertTrue(any(line.startswith("dist/bundle.tgz:") for line in found), found)

    def test_a_stale_allowlist_entry_fails(self):
        root = self.repository()
        with patch.dict(check_repo_files.ALLOWED_FILES, {"gone/clip.mp4": "demo"}, clear=True):
            found = tree_problems(root)
        self.assertEqual(len(found), 1)
        self.assertIn("stale ALLOWED_FILES entry", found[0])

    def test_history_check_finds_a_file_added_and_removed_in_the_range(self):
        root = self.repository()
        base = git(root, "rev-parse", "HEAD")
        self.commit(root, {"pdesk.tgz": b"\x1f\x8b\x08\x00data"}, "add archive")
        git(root, "rm", "-q", "pdesk.tgz")
        git(root, "commit", "-qm", "remove archive")
        with patch.dict(check_repo_files.ALLOWED_FILES, {}, clear=True):
            self.assertEqual(tree_problems(root), [])
        found = history_problems(root, base)
        self.assertTrue(found)
        self.assertTrue(all("deleted later in the range" in line for line in found), found)

    def test_history_check_ignores_files_added_before_the_base(self):
        root = self.repository()
        self.commit(root, {"old.tgz": b"\x1f\x8b\x08\x00data"}, "old")
        base = git(root, "rev-parse", "HEAD")
        self.commit(root, {"src/new.rs": b"fn main() {}\n"}, "new")
        self.assertEqual(history_problems(root, base), [])


if __name__ == "__main__":
    unittest.main()
