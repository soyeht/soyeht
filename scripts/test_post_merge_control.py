"""The control run: before any commit is blamed, the failing step runs on the last green main on
this host. A step that fails there too is the host's failure, and the search never starts."""

import calendar
import json
import os
import time
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
        self.assertEqual(len(self.runs()), 4, "the run, its rerun, the control and its rerun: no search")
        self.assertEqual(self.record(self.shas[0])["result"], "pass")
        green = json.loads((self.health / "last_green.json").read_text())
        self.assertEqual(green["sha"], self.shas[0], "a host failure never moves the green marker")
        self.assertIn("the host, not a commit", post_merge_records.report(record))

    def test_a_control_that_fails_once_and_then_passes_still_finds_the_real_culprit(self):
        # The step flakes on the last green commit while a real regression sits in the range.
        once = self.root.parent / "control-once"
        once.write_text("")
        self.add("two", {"b": "2"})
        culprit = self.add("breaks", {"broken": "x"})
        head = self.add("four", {"c": "3"})
        with mock.patch.dict(os.environ, {"FAKE_CONTROL_ONCE": str(once)}):
            self.assertEqual(self.run_main(head), 1)
        record = self.record(head)
        self.assertFalse(once.exists(), "the control failed once")
        self.assertEqual(record["result"], "fail")
        self.assertEqual(record["culprit"]["sha"], culprit)
        self.assertEqual(record["control"], {"sha": self.shas[0], "result": "good", "flaked": True})
        self.assertIn("failed once and passed when run again", record["note"])

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
        self.assertIn("new gate", record["note"], "the commits since stay in the note")
        self.assertIn("breaks", record["note"])
        self.assertEqual(record["result"], "fail")

    def test_a_range_with_no_commit_says_so_and_does_not_claim_there_is_no_green_main(self):
        head = self.add("breaks", {"broken": "x"})
        self.assertEqual(self.run_main(head, "--since", head), 1)
        record = self.record(head)
        self.assertNotIn("no known green main", record["note"])
        self.assertIn("no commit between the last green main", record["note"])
        self.assertNotIn("control", record)

    def test_a_host_run_is_read_again_until_it_is_due_and_then_run_again(self):
        head = self.add("only change", {"b": "2"})
        self.run_on_a_broken_host(head)
        runs = len(self.runs())
        self.assertEqual(self.run_main(head), 0)  # not due: only the record is read
        self.assertEqual(len(self.runs()), runs)
        self.assertEqual(self.record(head)["result"], post_merge_records.HOST_RESULT)
        with mock.patch.object(post_merge_records, "HOST_RETRY_SECONDS", 0):  # due; the host is fixed
            self.assertEqual(self.run_main(head), 0)
        self.assertGreater(len(self.runs()), runs)
        self.assertEqual(self.record(head)["result"], "pass")
        self.assertEqual(self.record(head)["previous"]["result"], post_merge_records.HOST_RESULT)
        self.assertEqual(json.loads((self.health / "last_green.json").read_text())["sha"], head)


class HostRetryTests(unittest.TestCase):
    def test_only_a_host_record_is_due_and_only_after_the_wait(self):
        started = "2026-11-10T10:00:00Z"
        origin = calendar.timegm(time.strptime(started, "%Y-%m-%dT%H:%M:%SZ"))
        record = {"result": post_merge_records.HOST_RESULT, "started_at": started}
        wait = post_merge_records.HOST_RETRY_SECONDS
        self.assertFalse(post_merge_records.host_retry_due(record, origin + wait - 1))
        self.assertTrue(post_merge_records.host_retry_due(record, origin + wait))
        self.assertTrue(post_merge_records.host_retry_due({"result": post_merge_records.HOST_RESULT}, origin))
        for result in ("fail", "flaky", "pass"):
            self.assertFalse(post_merge_records.host_retry_due({**record, "result": result}, origin + 10 * wait))


if __name__ == "__main__":
    unittest.main()
