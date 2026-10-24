import json
import os
import shutil
import unittest
from unittest import mock

import hardening_gate
import test_post_merge as fixtures

WAIVER = "Verification change: ignored one slow test"


class VerificationWaiverTests(fixtures.SyntheticRepositoryCase):
    """The waiver of a squash-merged pull request is in its commit message, and the gate that
    runs after the merge reads it from there (hardening_gate.waiver, the one definition)."""

    def setUp(self):
        super().setUp()
        self.events = self.root.parent / "events"
        self.events.mkdir()
        environment = mock.patch.dict(os.environ, {"FAKE_EVENTS": str(self.events)})
        environment.start()
        self.addCleanup(environment.stop)

    def waived(self, sha) -> bool:
        return hardening_gate.waiver(self.events / f"{sha}.json")

    def test_the_gate_reads_the_waiver_from_the_commit_message(self):
        with_line = self.add(f"Add a slow test (#1)\n\nIgnore it by default.\n\n{WAIVER}\n", {"b": "2"})
        self.assertEqual(self.run_main(with_line), 0)
        self.assertTrue(self.waived(with_line))
        event = json.loads((self.events / f"{with_line}.json").read_text())["pull_request"]
        self.assertEqual(event["title"], "Add a slow test (#1)")
        self.assertIn(WAIVER, event["body"].splitlines())

    def test_a_commit_message_without_the_line_gives_no_waiver(self):
        self.add("Add a slow test (#1)\n\nIgnore it by default.\n", {"b": "2"})
        without_line = self.add("Say that Verification change: is not needed (#2)", {"c": "3"})
        for sha in (self.shas[0], self.shas[1], without_line):
            self.assertEqual(self.run_main(sha), 0)
            self.assertFalse(self.waived(sha))

    def test_a_commit_message_that_is_only_a_subject_has_an_empty_body(self):
        subject_only = self.add("Only a subject", {"b": "2"})
        self.assertEqual(self.run_main(subject_only), 0)
        event = json.loads((self.events / f"{subject_only}.json").read_text())
        self.assertEqual(event, {"pull_request": {"title": "Only a subject", "body": ""}})

    def test_the_culprit_search_gives_each_probe_the_message_of_its_own_commit(self):
        self.assertEqual(self.run_main(self.shas[0]), 0)
        waived = [self.add(f"waived {n}\n\n{WAIVER}", {f"w{n}": "1"}) for n in range(2)]
        self.add("plain", {"p": "1"})
        culprit = self.add("breaks the unit step\n\nbody", {"broken": "x"})
        head = self.add("head", {"h": "1"})
        shutil.rmtree(self.events)
        self.events.mkdir()
        self.assertEqual(self.run_main(head), 1)
        self.assertEqual(self.record(head)["culprit"]["sha"], culprit)
        probed = {path.stem for path in self.events.glob("*.json")}
        self.assertTrue(probed - {head}, "the search probed no earlier commit")
        for sha in probed:
            self.assertEqual(self.waived(sha), sha in waived,
                             fixtures.git(self.root, "log", "-1", "--format=%s", sha))


if __name__ == "__main__":
    unittest.main()
