"""`release_flow.py prepare`: the dry run says what the real run does, a refusal writes nothing, and a
changelog section a person finished is kept while the wire contract is still recorded."""

import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

import changelog
import release
import release_flow
from test_release_flow import DRAFT, NOTES, VERSION, FlowCase

SNAPSHOT = release.ROOT / "contracts/released/protocol-v1.json"


class PrepareTests(FlowCase):
    def setUp(self):
        super().setUp()
        self.main = self.patched(mock.patch.object(release, "main", return_value=0))
        self.draft = self.patched(mock.patch.object(changelog, "main", return_value=0))
        self.plan = self.patched(mock.patch.object(release, "snapshot_plan", return_value=SNAPSHOT))

    def patched(self, patch):
        started = patch.start()
        self.addCleanup(patch.stop)
        return started

    def prepare(self, target="current", dry_run=False):
        return self.quiet(release_flow.prepare, SimpleNamespace(target=target, dry_run=dry_run))

    def nothing_was_written(self):
        self.main.assert_not_called()
        self.draft.assert_not_called()

    def test_prepare_bumps_then_records_the_contract_then_drafts_the_target_version(self):
        target = str(release.next_release(release.current(), "minor").version)
        self.prepare("minor")
        self.assertEqual([call.args[0] for call in self.main.call_args_list], [["bump", "minor"], ["snapshot"]])
        self.draft.assert_called_with(["draft", "--version", target, "--replace"])

    def test_a_dry_run_runs_nothing_that_writes(self):
        target = str(release.next_release(release.current(), "minor").version)
        _, out = self.prepare("minor", dry_run=True)
        self.main.assert_called_once_with(["bump", "minor", "--dry-run"])
        self.draft.assert_called_once_with(["draft", "--version", target, "--print"])  # read once, shown from that
        self.assertIn("would be recorded in contracts/released/protocol-v1.json", out)

    def test_a_finished_section_is_kept_and_the_contract_is_still_recorded(self):
        _, out = self.prepare()
        self.main.assert_called_once_with(["snapshot"])
        self.draft.assert_not_called()
        self.assertIn(f"finished section of {VERSION}: kept as it is", out)
        self.assertNotIn("edit CHANGELOG.md", out)

    def test_the_dry_run_of_a_finished_section_says_the_same_thing(self):
        _, out = self.prepare(dry_run=True)
        self.nothing_was_written()
        self.assertIn(f"finished section of {VERSION}; it would be kept", out)
        self.assertIn("would be recorded", out)

    def test_a_draft_section_is_written_again_and_a_dry_run_shows_it(self):
        self.write_changelog(DRAFT)
        self.prepare()
        self.draft.assert_called_with(["draft", "--version", VERSION, "--replace"])
        self.draft.reset_mock()
        self.prepare(dry_run=True)
        self.draft.assert_called_once_with(["draft", "--version", VERSION, "--print"])

    def test_a_recorded_contract_is_reported_and_not_written_again(self):
        self.plan.return_value = None
        _, out = self.prepare(dry_run=True)
        self.assertIn("is recorded already", out)

    def test_a_finished_section_with_no_entry_is_refused_by_both_before_anything_is_written(self):
        self.write_changelog(f"## [{VERSION}] - 2026-11-01\n\nNothing yet.\n\n")
        for dry_run in (True, False):
            with self.assertRaisesRegex(changelog.ChangelogError, "has no entry"):
                self.prepare(dry_run=dry_run)
        self.nothing_was_written()

    def test_a_contract_that_cannot_be_recorded_is_refused_by_both_before_the_version_is_raised(self):
        self.plan.side_effect = ValueError("protocol-v1.json exists and differs")
        for dry_run in (True, False):
            with self.assertRaisesRegex(release_flow.Refused, "could not be recorded: protocol-v1.json exists"):
                self.prepare("minor", dry_run=dry_run)
        self.nothing_was_written()

    def test_a_version_that_cannot_be_raised_is_refused_before_anything_is_written(self):
        for dry_run in (True, False):
            with self.assertRaises(release_flow.Refused):
                self.prepare("not-a-version", dry_run=dry_run)
        self.nothing_was_written()

    def test_a_draft_that_cannot_be_made_stops_before_the_bump_and_the_contract(self):
        self.draft.return_value = 1  # for example `git log` of a shallow clone
        for dry_run in (True, False):
            with self.assertRaisesRegex(release_flow.Refused, "nothing was written"):
                self.prepare("minor", dry_run=dry_run)
        self.main.assert_not_called()
        self.assertEqual([call.args[0][-1] for call in self.draft.call_args_list], ["--print"] * 2)  # one a run

    def test_the_draft_is_read_first_in_print_mode_and_not_shown(self):
        self.draft.side_effect = lambda argv: print("DRAFT TEXT") or 0
        _, out = self.prepare("minor")
        self.assertEqual(out.count("DRAFT TEXT"), 1)  # the write call's own output, not the first read's
        self.assertEqual([call.args[0][-1] for call in self.draft.call_args_list], ["--print", "--replace"])

    def test_a_failing_bump_stops_before_the_contract_and_the_changelog(self):
        self.main.return_value = 1
        with self.assertRaisesRegex(release_flow.Refused, "could not be raised"):
            self.prepare("minor")
        self.main.assert_called_once()
        self.assertEqual([call.args[0][-1] for call in self.draft.call_args_list], ["--print"])  # nothing written

    def test_the_notes_of_the_kept_section_are_the_ones_a_release_publishes(self):
        text = Path(changelog.CHANGELOG).read_text()
        self.assertTrue(changelog.is_final(text, VERSION))
        self.assertEqual(changelog.notes_of(text, VERSION), NOTES.split("\n", 1)[1].strip() + "\n")
        self.assertFalse(changelog.is_final(DRAFT, VERSION))
        self.assertFalse(changelog.is_final(text, "9.9.9"))


if __name__ == "__main__":
    unittest.main()
