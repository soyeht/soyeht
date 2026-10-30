import contextlib
import io
import tempfile
import unittest
from datetime import date
from pathlib import Path
from types import SimpleNamespace

import changelog
import release_manifest

START = "# Changelog\n\n## [Unreleased]\n\n"
OLDER = "## [0.1.0] - 2026-10-01\n\n### Added\n\n- The first thing.\n"


def subjects(*lines):
    return list(lines)


class ChangelogTests(unittest.TestCase):
    def test_a_subject_becomes_an_entry_with_its_pull_request_as_a_link(self):
        category, line = changelog.entry("Add the web pane on Windows, on WebView2 (#20)")
        self.assertEqual(category, "Added")
        self.assertEqual(line, "- Add the web pane on Windows, on WebView2 ([#20](https://github.com/soyeht/soyeht/pull/20)).")
        self.assertEqual(changelog.entry("Fix the flaky terminal drain acknowledgement test (#12)")[0], "Fixed")
        self.assertEqual(changelog.entry("Remove the old socket path")[0], "Removed")
        self.assertEqual(changelog.entry("Harden the installer's helpers (#22)")[0], "Security")
        self.assertEqual(changelog.entry("Split post_merge.py into modules (#4)")[0], "Changed")
        self.assertEqual(changelog.entry("No pull request here")[1], "- No pull request here.")

    def test_the_draft_groups_entries_by_category_in_the_order_of_the_format_and_marks_itself(self):
        text = changelog.render("0.2.0", "2026-11-01", subjects("Fix a (#2)", "Add b (#3)", "Rework c (#4)"), "v0.1.0")
        self.assertTrue(text.startswith("## [0.2.0] - 2026-11-01\n"))
        self.assertIn(changelog.DRAFT_MARK, text)
        self.assertIn("3 merged pull requests since v0.1.0", text)
        self.assertLess(text.index("### Added"), text.index("### Changed"))
        self.assertLess(text.index("### Changed"), text.index("### Fixed"))
        self.assertNotIn("### Removed", text)

    def test_the_first_release_has_a_skeleton_to_write_by_hand(self):
        text = changelog.render("0.1.0", "2026-11-01", [], None)
        self.assertIn(changelog.DRAFT_MARK, text)
        self.assertIn("### Added", text)

    def test_a_section_goes_below_unreleased_and_above_the_older_ones(self):
        section = changelog.render("0.2.0", "2026-11-01", ["Add b (#3)"], "v0.1.0")
        text = changelog.insert(START + OLDER, section, "0.2.0", False)
        names = [name for name, _, _ in changelog.sections(text)]
        self.assertEqual(names, ["Unreleased", "0.2.0", "0.1.0"])
        self.assertTrue(text.endswith(OLDER))
        alone = changelog.insert(START, section, "0.2.0", False)
        self.assertEqual([name for name, _, _ in changelog.sections(alone)], ["Unreleased", "0.2.0"])
        self.assertTrue(alone.endswith("\n") and not alone.endswith("\n\n"))

    def test_a_version_already_written_is_kept_unless_replaced(self):
        text = START + OLDER
        with self.assertRaises(changelog.ChangelogError):
            changelog.insert(text, "## [0.1.0] - 2026-11-01\n", "0.1.0", False)
        again = changelog.insert(text, "## [0.1.0] - 2026-11-01\n\n- Different.\n", "0.1.0", True, force=True)
        self.assertEqual([name for name, _, _ in changelog.sections(again)], ["Unreleased", "0.1.0"])
        self.assertNotIn("The first thing", again)

    def test_replace_writes_over_a_draft_but_never_over_an_edited_section(self):
        draft = changelog.render("0.2.0", "2026-11-01", ["Add b (#3)"], "v0.1.0")
        text = changelog.insert(START, draft, "0.2.0", False)
        again = changelog.insert(text, changelog.render("0.2.0", "2026-11-02", ["Add c (#4)"], "v0.1.0"), "0.2.0", True)
        self.assertIn("Add c", again)
        self.assertNotIn("Add b", again)
        edited = text.replace(changelog.DRAFT_MARK, "<!-- gone:").replace("<!-- gone:", "")
        with self.assertRaisesRegex(changelog.ChangelogError, "has been edited"):
            changelog.insert(edited, draft, "0.2.0", True)
        self.assertIn("Add b", changelog.insert(edited, draft, "0.2.0", True, force=True))

    def test_the_notes_are_the_section_without_its_heading_and_a_draft_is_refused(self):
        self.assertEqual(changelog.notes_of(START + OLDER, "0.1.0"), "### Added\n\n- The first thing.\n")
        draft = changelog.insert(START, changelog.render("0.2.0", "2026-11-01", ["Add b (#3)"], "v0.1.0"), "0.2.0", False)
        with self.assertRaisesRegex(changelog.ChangelogError, "still a draft"):
            changelog.notes_of(draft, "0.2.0")
        with self.assertRaisesRegex(changelog.ChangelogError, "no section"):
            changelog.notes_of(START, "0.2.0")
        with self.assertRaisesRegex(changelog.ChangelogError, "no entry"):
            changelog.notes_of(START + "## [0.3.0] - 2026-11-01\n\n### Added\n\n", "0.3.0")

    def test_unreleased_entries_are_noticed(self):
        self.assertFalse(changelog.unreleased_has_entries(START + OLDER))
        self.assertTrue(changelog.unreleased_has_entries("## [Unreleased]\n\n- Something.\n\n" + OLDER))

    def test_the_real_changelog_keeps_the_structure_of_the_format(self):
        text = changelog.CHANGELOG.read_text()
        self.assertTrue(text.startswith("# Changelog"))
        self.assertIsNotNone(changelog.section_of(text, changelog.UNRELEASED))
        self.assertIn("keepachangelog.com", text)

    def test_the_release_notes_address_of_the_contract_is_the_release_the_notes_are_published_in(self):
        notes = release_manifest.POLICY["release"]["notesUrl"]
        self.assertTrue(notes.startswith(changelog.HOMEPAGE + "/releases/tag/"))


class GitTests(unittest.TestCase):
    """The draft from a real repository: squash subjects on the first-parent line after a tag."""

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.git("init", "-q", "-b", "main")
        (self.root / "CHANGELOG.md").write_text(START + OLDER)
        self.commit("Start the project (#1)")

    def git(self, *arguments):
        import subprocess

        subprocess.run(
            ["git", "-c", "user.name=T", "-c", "user.email=t@example.com", "-c", "tag.gpgsign=false",
             "-c", "commit.gpgsign=false", *arguments],
            cwd=self.root, check=True, capture_output=True,
        )

    def commit(self, subject):
        self.git("commit", "-q", "--allow-empty", "-m", subject)

    def test_the_commits_after_the_last_tag_are_listed_newest_first(self):
        self.assertIsNone(changelog.last_tag(self.root))
        self.git("tag", "v0.1.0")
        self.commit("Add one (#2)")
        self.commit("Fix two (#3)")
        self.assertEqual(changelog.last_tag(self.root), "v0.1.0")
        self.assertEqual(changelog.squash_subjects("v0.1.0", self.root), ["Fix two (#3)", "Add one (#2)"])

    def test_draft_writes_the_section_to_the_file_and_never_tags_or_commits(self):
        self.git("tag", "v0.1.0")
        self.commit("Add one (#2)")
        before = changelog.git("rev-parse", "HEAD", root=self.root)
        arguments = SimpleNamespace(version="0.2.0", since=None, date=None, replace=False, force=False, print=False)
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(changelog.draft_command(arguments, self.root, date(2026, 11, 1)), 0)
        text = (self.root / "CHANGELOG.md").read_text()
        self.assertIn("## [0.2.0] - 2026-11-01", text)
        self.assertIn("- Add one", text)
        self.assertEqual(changelog.git("rev-parse", "HEAD", root=self.root), before)
        self.assertEqual(changelog.git("tag", root=self.root).split(), ["v0.1.0"])
        with self.assertRaises(changelog.ChangelogError):
            changelog.draft_command(arguments, self.root, date(2026, 11, 1))


if __name__ == "__main__":
    unittest.main()
