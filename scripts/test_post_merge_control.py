"""The control run: before any commit is blamed, the failing step runs on the last green main on
this host. A step that fails there too is the host's failure, and the search never starts."""

import json
import os
import unittest
from unittest import mock

import post_merge_gate
import post_merge_records
from test_post_merge_fixtures import FAKE_GATE, SyntheticRepositoryCase


class ControlRunTests(SyntheticRepositoryCase):
    def setUp(self):
        super().setUp()
        self.trace = self.root.parent / "trace.txt"
        self.enterContext(mock.patch.dict(os.environ, {"FAKE_TRACE": str(self.trace)}))
        self.assertEqual(self.run_main(self.shas[0]), 0)  # the last green main
        self.trace.unlink()

    def runs(self):
        """What the gate was run with, each line: the full run, a rerun, the control, each probe."""
        return [line.rsplit("only=", 1)[1].split()[0] for line in self.trace.read_text().splitlines()]

    def run_on_a_broken_host(self, head):
        """The record of a run in which the step fails on every commit: no commit is to blame."""
        with mock.patch.dict(os.environ, {"FAKE_HOST_BROKEN": "1"}):
            self.assertEqual(self.run_main(head), 1)
        record = self.record(head)
        self.assertEqual(record["result"], post_merge_records.HOST_RESULT)
        self.assertNotIn("culprit", record)
        return record

    def test_a_step_that_fails_on_every_commit_is_the_hosts_and_blames_nobody(self):
        self.add("two", {"b": "2"})
        self.add("three", {"c": "3"})
        head = self.add("four", {"d": "4"})
        record = self.run_on_a_broken_host(head)
        self.assertEqual(record["failed_step"], "unit")
        self.assertNotIn("candidates", record)
        self.assertEqual(record["control"], {"sha": self.shas[0], "result": "bad"})
        self.assertIn(f"fails on the last green main {self.shas[0][:12]} too", record["note"])
        self.assertEqual(len(self.runs()), 3, "the run, its rerun and the control: no search")
        self.assertEqual(self.record(self.shas[0])["result"], "pass")
        green = json.loads((self.health / "last_green.json").read_text())
        self.assertEqual(green["sha"], self.shas[0], "a host failure never moves the green marker")
        self.assertIn("the host, not a commit", post_merge_records.report(record))

    def test_a_single_commit_range_is_probed_too(self):
        head = self.add("breaks", {"broken": "x"})
        self.assertEqual(self.run_main(head), 1)
        record = self.record(head)
        self.assertEqual(record["culprit"]["sha"], head)
        self.assertEqual(record["control"], {"sha": self.shas[0], "result": "good"})
        self.assertEqual(len(self.runs()), 3, "the run, its rerun and the control on the last green")

    def test_a_single_commit_range_is_not_blamed_when_the_host_fails(self):
        head = self.add("only change", {"b": "2"})
        record = self.run_on_a_broken_host(head)
        self.assertEqual(record["control"]["sha"], self.shas[0])

    def test_a_real_culprit_is_still_found_after_the_control_passes(self):
        self.add("two", {"b": "2"})
        culprit = self.add("breaks", {"broken": "x"})
        self.add("four", {"c": "3"})
        head = self.add("five", {"d": "4"})
        self.assertEqual(self.run_main(head), 1)
        record = self.record(head)
        self.assertEqual(record["result"], "fail")
        self.assertEqual(record["culprit"]["sha"], culprit)
        self.assertEqual(record["control"]["result"], "good")
        self.assertGreater(len(self.runs()), 3, "the control, then the binary search")

    def test_the_control_takes_the_prerequisites_of_the_step(self):
        head = self.add("breaks", {"broken": "x"})
        with mock.patch.dict(os.environ, {"FAKE_STEP": "tcc"}), \
                mock.patch.object(post_merge_gate, "PREREQUISITES", {"tcc": ("snap",)}):
            self.assertEqual(self.run_main(head), 1)
        self.assertEqual(self.record(head)["culprit"]["sha"], head)
        # The rerun and the control, both with the snapshot the step compares with (the run itself is first).
        self.assertEqual(self.runs()[1:], ["snap,tcc", "snap,tcc"])

    def test_a_control_that_cannot_run_blames_nobody(self):
        # The last green commit has a gate without `--only`: the step cannot be run alone there.
        self.add("old gate", {"scripts/check_all.sh": FAKE_GATE.replace("--only", "--o_n_l_y")})
        self.assertEqual(self.run_main(self.shas[-1]), 0)
        self.add("new gate", {"scripts/check_all.sh": FAKE_GATE})
        head = self.add("breaks", {"broken": "x"})
        self.assertEqual(self.run_main(head), 1)
        record = self.record(head)
        self.assertEqual(record["control"], {"sha": self.shas[1], "result": "skip"})
        self.assertNotIn("culprit", record)
        self.assertNotIn("candidates", record)
        self.assertIn("could not be run on the last green main", record["note"])
        self.assertEqual(record["result"], "fail")


if __name__ == "__main__":
    unittest.main()
