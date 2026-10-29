import json
import os
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import gate_flakes
import post_merge_gate
from test_post_merge_fixtures import FAKE_ONLY_ARGUMENTS, FAKE_RECORD_STEP, SyntheticRepositoryCase


# A stand-in with three steps, `a`, `b` and `c`, run in that order. `a` fails once while the file
# FAKE_ONCE exists (the failure removes it); `c` fails while the commit has a file `broken` and once
# while FAKE_ONCE_C exists, and in a selection of several steps when FAKE_FAIL_IN_BATCH is set; the
# gate exits 71 (cannot run) for a selection that names the step FAKE_INFRA_ON, and writes no step
# times for a selection when FAKE_NO_TIMINGS_FOR_SELECTIONS is set.
FAKE_TAIL_GATE = """#!/bin/bash
# supports --only
""" + FAKE_ONLY_ARGUMENTS + """if ((list)); then printf 'always both a\nswift both b\nswift both c\n'; exit 0; fi
[[ -z "${FAKE_TRACE:-}" ]] || echo "$PWD only=$only" >> "$FAKE_TRACE"
[[ -z "${FAKE_INFRA_ON:-}" || ",$only," != *",$FAKE_INFRA_ON,"* ]] || exit 71
[[ -z "${FAKE_NO_TIMINGS_FOR_SELECTIONS:-}" || -z "$only" ]] || export SOYEHT_GATE_TIMINGS=/dev/null
steps=""
for id in a b c; do
    [[ -z "$only" || ",$only," == *",$id,"* ]] || continue
    status=0
    if [[ "$id" == a && -n "${FAKE_ONCE:-}" && -e "$FAKE_ONCE" ]]; then rm -f "$FAKE_ONCE"; status=1
    elif [[ "$id" == c && -e broken ]]; then status=1
    elif [[ "$id" == c && -n "${FAKE_ONCE_C:-}" && -e "$FAKE_ONCE_C" ]]; then rm -f "$FAKE_ONCE_C"; status=1
    elif [[ "$id" == c && -n "${FAKE_FAIL_IN_BATCH:-}" && "$only" == *,* ]]; then status=1; fi
""" + FAKE_RECORD_STEP


class FlakyTailTests(SyntheticRepositoryCase):
    """A flaky step stops the first run: the steps after it still run, and a flake is remembered."""

    FILES = {"scripts/check_all.sh": FAKE_TAIL_GATE, "a": "1"}

    def setUp(self):
        super().setUp()
        self.trace = self.root.parent / "trace.txt"
        self.once = self.root.parent / "once"
        environment = mock.patch.dict(os.environ, {
            "FAKE_TRACE": str(self.trace), "FAKE_ONCE": str(self.once)})
        environment.start()
        self.addCleanup(environment.stop)
        self.assertEqual(self.run_main(self.shas[0]), 0)
        self.trace.unlink()

    def selections(self):
        return [line.rsplit("only=", 1)[1] for line in self.trace.read_text().splitlines()]

    def flake_on_the_next_run(self, files=None):
        head = self.add("two", files or {"b": "2"})
        self.once.write_text("")
        return head

    def test_the_steps_after_a_flaky_step_run_and_the_record_says_they_passed(self):
        head = self.flake_on_the_next_run()
        self.assertEqual(self.run_main(head), 1)
        record = self.record(head)
        self.assertEqual(record["result"], "flaky")
        self.assertEqual(self.selections(), ["", "a", "b,c"], "the gate, the rerun, the steps it never reached")
        self.assertEqual([step["id"] for step in record["steps"]], ["a", "b", "c"])
        self.assertIn("steps after it ran and passed", record["note"])
        self.assertEqual([flake["step"] for flake in record["flaky_steps"]], ["a"])
        self.assertNotIn(head, (self.health / "last_green.json").read_text(), "a flaky run is not green")

    def test_a_later_step_that_fails_is_judged_in_its_turn_and_names_the_culprit(self):
        self.add("two", {"b": "2"})
        culprit = self.add("breaks c", {"broken": "x"})
        head = self.add("four", {"c": "3"})
        self.once.write_text("")
        self.assertEqual(self.run_main(head, "--since", self.shas[0]), 1)
        record = self.record(head)
        self.assertEqual((record["result"], record["failed_step"]), ("fail", "c"))
        self.assertEqual(record["culprit"]["sha"], culprit)
        self.assertEqual([flake["step"] for flake in record["flaky_steps"]], ["a"])
        self.assertIn("a flaked earlier in this run", record["note"])

    def flaky_record(self, head, **environment):
        """The record of a run whose `a` flakes, under `environment`."""
        with mock.patch.dict(os.environ, environment):
            self.assertEqual(self.run_main(head), 1)
        record = self.record(head)
        self.assertEqual(record["result"], "flaky")
        self.assertEqual([flake["step"] for flake in record["flaky_steps"]][:1], ["a"])
        return record

    def test_a_later_step_that_flakes_too_is_recorded_and_the_rest_still_runs(self):
        head = self.flake_on_the_next_run()
        once_c = self.root.parent / "once-c"
        once_c.write_text("")  # `c` fails once as well, in the steps after the flaky `a`
        record = self.flaky_record(head, FAKE_ONCE_C=str(once_c))
        self.assertEqual([flake["step"] for flake in record["flaky_steps"]], ["a", "c"])
        self.assertEqual(self.selections(), ["", "a", "b,c", "c"])
        self.assertIn("step a, c passed when run again", record["note"])

    def test_a_step_that_fails_in_the_batch_and_passes_alone_ends_the_run(self):
        head = self.flake_on_the_next_run()
        # No step times for a selection, so the tail records nothing and `c` is found from the log;
        # it fails in `b,c` and passes alone, which would repeat for ever.
        record = self.flaky_record(head, FAKE_FAIL_IN_BATCH="1", FAKE_NO_TIMINGS_FOR_SELECTIONS="1")
        self.assertEqual([flake["step"] for flake in record["flaky_steps"]], ["a", "c"])
        self.assertIn("fails in the full run and passes when run alone", record["note"])
        self.assertLessEqual(len(self.selections()), 6)

    def test_a_prerequisite_that_already_ran_is_not_taken_again(self):
        head = self.flake_on_the_next_run()
        with mock.patch.object(post_merge_gate, "PREREQUISITES", {"c": "a"}):
            self.assertEqual(self.run_main(head), 1)
        self.assertEqual(self.selections(), ["", "a", "b,c"], "`a` ran: its snapshot is not retaken before `c`")
        self.assertEqual(self.record(head)["result"], "flaky")

    def test_steps_that_cannot_run_after_a_flake_are_reported_as_unverified(self):
        head = self.flake_on_the_next_run()
        with mock.patch.dict(os.environ, {"FAKE_INFRA_ON": "b"}):
            self.assertEqual(self.run_main(head), 1)
        record = self.record(head)
        self.assertEqual(record["result"], "flaky")
        self.assertIn("could not be run (exit 71) and are unverified", record["note"])
        self.assertEqual([flake["step"] for flake in record["flaky_steps"]], ["a"])

    def test_a_flake_is_written_to_the_ledger_with_its_sha_load_and_date(self):
        head = self.flake_on_the_next_run()
        self.assertEqual(self.run_main(head), 1)
        ledger = json.loads((self.health / "flakes.json").read_text())
        self.assertEqual([run["sha"] for run in ledger["runs"]], [self.shas[0], head])
        (flake,) = ledger["flakes"]
        self.assertEqual((flake["step"], flake["sha"], flake["date"]), ("a", head, self.record(head)["started_at"]))
        self.assertEqual(flake["load"], self.record(head)["local_load"])
        self.assertEqual(flake["kind"], "passed-on-rerun")


class FlakeLedgerTests(unittest.TestCase):
    """`flakes.json`: a step with two flakes in the last ten runs is chronic."""

    def setUp(self):
        self.health = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.health, ignore_errors=True)
        self.count = 0

    def run_with(self, *flaky, sha=None):
        self.count += 1
        record = {"sha": sha or f"{self.count:040x}", "started_at": f"2026-10-{self.count:02d}T00:00:00Z",
                  "flaky_steps": [gate_flakes.flaky_step(step, {"local_load": {"average": 3.0}}, None) for step in flaky]}
        gate_flakes.record_run(self.health, record)
        return record["sha"]

    def test_two_flakes_of_one_step_in_ten_runs_are_chronic(self):
        self.run_with("swift:test-shared")
        for _ in range(4):
            self.run_with()
        self.run_with("swift:test-shared", "engine:timing-frame-dense")
        self.assertEqual(gate_flakes.chronic_steps(self.health), [
            {"step": "swift:test-shared", "flakes": 2, "runs": 6,
             "shas": [f"{1:040x}", f"{6:040x}"]}])

    def test_a_flake_that_left_the_window_is_forgotten(self):
        self.run_with("a")
        self.run_with("a")
        self.assertEqual([c["step"] for c in gate_flakes.chronic_steps(self.health)], ["a"])
        for _ in range(gate_flakes.CHRONIC_WINDOW - 1):
            self.run_with()  # the first flake is now eleven runs back
        self.assertEqual(gate_flakes.chronic_steps(self.health), [])

    def test_a_run_again_of_a_commit_replaces_its_entries(self):
        sha = self.run_with("a")
        self.run_with("a", sha=sha)
        ledger = gate_flakes.read_ledger(self.health)
        self.assertEqual((len(ledger["runs"]), len(ledger["flakes"])), (1, 1))
        self.assertEqual(gate_flakes.chronic_steps(self.health), [])

    def test_the_ledger_is_bounded(self):
        for _ in range(gate_flakes.RUN_HISTORY + 5):
            self.run_with("a")
        ledger = gate_flakes.read_ledger(self.health)
        self.assertEqual((len(ledger["runs"]), len(ledger["flakes"])), (gate_flakes.RUN_HISTORY,) * 2)

    def test_a_missing_or_corrupt_ledger_is_empty(self):
        self.assertEqual(gate_flakes.chronic_steps(self.health), [])
        (self.health / gate_flakes.LEDGER_FILE).write_text("{not json")
        self.assertEqual(gate_flakes.read_ledger(self.health), {"runs": [], "flakes": []})

    def test_a_ledger_of_the_wrong_shape_is_empty_and_is_replaced(self):
        for content in ("null", "[]", '{"runs":[1]}', '{"runs":{"a":1},"flakes":"x"}',
                        '{"runs":[{"sha":1}],"flakes":[1,{"step":3,"sha":"a"},{"step":"s"}]}'):
            with self.subTest(content=content):
                (self.health / gate_flakes.LEDGER_FILE).write_text(content)
                self.assertEqual(gate_flakes.read_ledger(self.health), {"runs": [], "flakes": []})
                self.assertEqual(gate_flakes.chronic_steps(self.health), [])
                self.run_with("a")
                self.assertEqual(len(gate_flakes.read_ledger(self.health)["flakes"]), 1)

    def test_the_ledger_is_replaced_atomically(self):
        self.run_with("a")
        with mock.patch.object(gate_flakes.os, "replace", side_effect=OSError("full")):
            with self.assertRaises(OSError):
                self.run_with("b")
        self.assertEqual([flake["step"] for flake in gate_flakes.read_ledger(self.health)["flakes"]], ["a"],
                         "a write that fails leaves the ledger as it was")

    def test_a_flake_carries_the_load_of_its_own_rerun_not_an_earlier_flakes(self):
        record = {"load": {"host": "linux-host", "average": 80.0, "cpus": 32},
                  "local_load": {"host": "local", "average": 3.0, "cpus": 8}}
        self.assertEqual(gate_flakes.flaky_step("swift:test-shared", record, None)["load"], record["local_load"])
        timing = {"host": "linux-host", "average": 1.0, "cpus": 32}
        self.assertEqual(gate_flakes.flaky_step("engine:timing-x", record, timing)["load"], timing)

    def test_the_steps_a_failed_run_never_reached(self):
        areas = {"a": "always", "b": "swift", "c": "swift"}
        self.assertEqual(gate_flakes.unrun_steps(areas, [{"id": "a"}]), ["b", "c"])
        self.assertEqual(gate_flakes.unrun_steps(areas, [{"id": "c"}, {"id": "a"}]), ["b"])
        self.assertIsNone(gate_flakes.unrun_steps(areas, []), "a run with no step times cannot say")
        self.assertIsNone(gate_flakes.unrun_steps(None, [{"id": "a"}]))


if __name__ == "__main__":
    unittest.main()
