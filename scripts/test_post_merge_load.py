import contextlib
import io
import os
import subprocess
import unittest
from pathlib import Path
from unittest import mock

import post_merge_hosts
from test_post_merge_fixtures import FAKE_ONLY_ARGUMENTS, RemoteHostCase


REAL_REMOTE_LOAD = post_merge_hosts.remote_load  # LoadRuleTests replaces the module's


# A stand-in with the three kinds of step the load rules tell apart: `unit` (no latency limit), `tm`
# (area `timing`) and the Rust editor step (area `engine`); with a host, `tm` and the editor step run
# there and `unit` here. A step named by FAKE_STEP fails while the file FAKE_NOISE exists (a loaded
# Mac), FAKE_HOST_NOISE exists (a loaded host), or `broken` does; and once, when FAKE_ONCE exists
# (it is removed by that failure). On the host (the checkout `remote-gate`) the Mac's noise is not heard.
FAKE_LOAD_GATE = """#!/bin/bash
# supports --only
""" + FAKE_ONLY_ARGUMENTS + """if ((list)); then printf 'always both unit\\ntiming full tm\\nengine full editor-performance:rust\\n'; exit 0; fi
step="${FAKE_STEP:-tm}"
echo "$PWD only=$only" >> "${FAKE_TRACE:?}"
[[ -z "$only" || ",$only," == *",$step,"* ]] || exit 0
status=0
[[ -e broken ]] && status=1
[[ -n "${FAKE_NOISE:-}" && -e "$FAKE_NOISE" && "$PWD" != *remote-gate* ]] && status=1
[[ -n "${FAKE_HOST_NOISE:-}" && -e "$FAKE_HOST_NOISE" && "$PWD" == *remote-gate* ]] && status=1
[[ -n "${FAKE_ONCE:-}" && -e "$FAKE_ONCE" ]] && { rm -f "$FAKE_ONCE"; status=1; }
printf '{"latest":{"steps":[{"id":"%s","area":"x","seconds":1.0,"status":%s}]}}' "$step" "$status" > "${SOYEHT_GATE_TIMINGS:?}"
((status)) && echo "check_all: FAILED step $step (area x, exit 1)" >&2
exit "$status"
"""

BUSY, QUIET = post_merge_hosts.Load("local", 80.0, 8), post_merge_hosts.Load("local", 2.0, 8)


class LoadRuleTests(RemoteHostCase):
    """A timing step that fails on a busy host is rerun on a quiet one before main is called red."""

    FILES = {
        "scripts/check_all.sh": FAKE_LOAD_GATE, "a": "1",
        "scripts/remote_gate.sh": (Path(__file__).with_name("remote_gate.sh")).read_text(),
    }

    def setUp(self):
        super().setUp()
        self.noise = self.root.parent / "noise"
        self.host_noise = self.root.parent / "host-noise"
        self.once = self.root.parent / "once"
        environment = mock.patch.dict(os.environ, {
            "FAKE_NOISE": str(self.noise), "FAKE_HOST_NOISE": str(self.host_noise), "FAKE_ONCE": str(self.once)})
        environment.start()
        self.addCleanup(environment.stop)
        self.pauses = []

        def pause(seconds):
            self.pauses.append(seconds)
            self.noise.unlink(missing_ok=True)  # the load drops while we wait
            self.host_noise.unlink(missing_ok=True)

        for target, replacement in (
            ("pause", pause),
            ("local_load", lambda: BUSY if self.noise.exists() else QUIET),
            ("remote_load", lambda remote: post_merge_hosts.Load(remote.host, 80.0 if self.host_noise.exists() else 1.0, 32)),
        ):
            patch = mock.patch.object(post_merge_hosts, target, replacement)
            patch.start()
            self.addCleanup(patch.stop)
        self.assertEqual(self.run_main(self.shas[0]), 0)
        self.trace.unlink()

    def commits_with_a_culprit(self):
        """Four commits after the start, the second makes the step fail: (that commit, the head)."""
        self.add("two", {"b": "2"})
        culprit = self.add("slows it down", {"broken": "x"})
        self.add("four", {"c": "3"})
        return culprit, self.add("five", {"d": "4"})

    def fail_under_load(self, step, *extra, on_host=False):
        self.add("two", {"b": "2"})
        head = self.add("three", {"c": "3"})
        (self.host_noise if on_host else self.noise).write_text("")
        with mock.patch.dict(os.environ, {"FAKE_STEP": step}):
            return head, self.run_main(head, *extra)

    def flaky_record_under_load(self, *extra, on_host=False):
        """The record of a timing step that failed on a busy host and passed on the rerun."""
        head, code = self.fail_under_load("tm", *extra, on_host=on_host)
        self.assertEqual(code, 1)
        record = self.record(head)
        self.assertEqual(record["result"], "flaky")
        return record

    def probe_record(self, head, local_load):
        """The record of a run whose culprit search reads the Mac's load from `local_load`."""
        with mock.patch.object(post_merge_hosts, "local_load", local_load), \
                mock.patch.dict(os.environ, {"FAKE_STEP": "tm"}):
            self.assertEqual(self.run_main(head, "--since", self.shas[0]), 1)
        return self.record(head)

    def test_a_timing_step_that_failed_on_a_busy_host_is_rerun_after_the_load_drops(self):
        record = self.flaky_record_under_load()
        self.assertEqual(record["load"], {"host": "local", "average": 80.0, "cpus": 8})
        self.assertEqual(record["load_before_rerun"]["average"], 2.0)
        self.assertIn("busy; rerun after waiting", record["note"])
        self.assertEqual(self.pauses, [post_merge_hosts.LOAD_POLL_SECONDS])
        self.assertNotIn("culprit", record)

    def test_a_timing_step_runs_on_the_host_and_is_rerun_there_after_its_load_drops(self):
        record = self.flaky_record_under_load(*self.remote_options(), on_host=True)
        self.assertEqual(record["load"], {"host": "linux-host", "average": 80.0, "cpus": 32})
        self.assertEqual(record["flaky_steps"][0]["load"], record["load"], "the ledger keeps the host's load")
        self.assertIn("busy; rerun after waiting", record["note"])
        self.assertEqual(self.pauses, [post_merge_hosts.LOAD_POLL_SECONDS])
        first, rerun, *tail = self.runs()
        self.assertTrue(all("remote-gate/tree" in directory and arguments.startswith("only=tm")
                            for directory, arguments in (first, rerun)))
        self.assertEqual(rerun[1], "only=tm")
        # What the failed run never reached: the host's step there, the Mac's step here.
        self.assertEqual([(("remote-gate" in directory), arguments.split()[0]) for directory, arguments in tail],
                         [(True, "only=editor-performance:rust"), (False, "only=unit")])

    def test_a_step_without_a_latency_limit_is_rerun_at_once_whatever_the_load(self):
        head, code = self.fail_under_load("unit")
        self.assertEqual(code, 1)
        record = self.record(head)
        self.assertEqual(self.pauses, [])
        self.assertNotIn("load", record)
        self.assertNotIn("load_before_rerun", record)
        self.assertEqual(record["local_load"]["host"], "local")  # every record says how loaded the Mac was

    def test_a_timing_step_that_fails_on_a_quiet_host_is_still_a_regression(self):
        self.add("two", {"b": "2"})
        culprit = self.add("slows it down", {"broken": "x"})
        head = self.add("four", {"c": "3"})
        with mock.patch.dict(os.environ, {"FAKE_STEP": "tm"}):
            self.assertEqual(self.run_main(head, "--since", self.shas[0]), 1)
        record = self.record(head)
        self.assertEqual((record["result"], record["culprit"]["sha"]), ("fail", culprit))
        self.assertEqual(record["load"]["average"], 2.0)
        self.assertEqual(self.pauses, [])

    def test_the_culprit_search_waits_for_a_quiet_host_before_each_probe_of_a_timing_step(self):
        culprit, head = self.commits_with_a_culprit()
        with mock.patch.dict(os.environ, {"FAKE_STEP": "tm"}), \
                mock.patch.object(post_merge_hosts, "wait_until_quiet", return_value=QUIET) as waited:
            self.assertEqual(self.run_main(head, "--since", self.shas[0]), 1)
        self.assertEqual(self.record(head)["culprit"]["sha"], culprit)
        # The gate, the rerun, then one run per probe; each probe was preceded by a wait.
        probes = len(self.trace.read_text().splitlines()) - 2
        self.assertGreaterEqual(probes, 2)
        self.assertEqual(waited.call_count, probes)

    def test_the_timing_steps_run_on_the_remote_host_with_the_engine_steps(self):
        head = self.add("two", {"b": "2"})
        with mock.patch.dict(os.environ, {"FAKE_STEP": "editor-performance:rust"}):
            self.assertEqual(self.run_remote(head), 0)
        (remote_directory, remote_arguments), (local_directory, local_arguments) = self.runs()
        self.assertIn("remote-gate/tree", remote_directory)
        self.assertEqual(remote_arguments.split()[0], "only=tm,editor-performance:rust")
        self.assertNotIn("remote-gate", local_directory)
        self.assertEqual(local_arguments.split()[0], "only=unit")

    def test_a_remote_step_is_judged_by_the_load_of_the_host_it_ran_on(self):
        self.add("two", {"b": "2"})
        head = self.add("three", {"c": "3"})
        self.noise.write_text("")  # the Mac is busy; the host does not hear it, and its own load is quiet
        with mock.patch.dict(os.environ, {"FAKE_STEP": "editor-performance:rust", "FAKE_NOISE": ""}):
            self.assertEqual(self.run_remote(head), 0)
        self.assertEqual(self.pauses, [])
        self.assertEqual(self.record(head)["local_load"]["average"], 80.0)

    def test_load_is_busy_per_processor_and_the_wait_is_bounded(self):
        self.assertTrue(post_merge_hosts.Load("h", 16.1, 16).busy())
        self.assertFalse(post_merge_hosts.Load("h", 16.0, 16).busy())
        with mock.patch.dict(os.environ, {post_merge_hosts.LOAD_LIMIT_ENV: "4"}):
            self.assertFalse(post_merge_hosts.Load("h", 60.0, 16).busy())
        waits = []
        budget = post_merge_hosts.WaitBudget()
        with mock.patch.object(post_merge_hosts, "pause", waits.append), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(post_merge_hosts.wait_until_quiet(lambda: BUSY, budget), BUSY)
            self.assertEqual(post_merge_hosts.wait_until_quiet(lambda: BUSY, budget), BUSY, "the budget is shared")
            self.assertIsNone(post_merge_hosts.wait_until_quiet(lambda: None, budget), "a host that cannot say is not waited for")
        self.assertEqual(sum(waits), post_merge_hosts.LOAD_WAIT_BUDGET_SECONDS)

    # The cases of the review of the first version: a host that never quiets down, or cannot be asked.

    def stay_busy(self):
        """The Mac never quiets down: waiting does not clear the noise."""
        patch = mock.patch.object(post_merge_hosts, "pause", self.pauses.append)
        patch.start()
        self.addCleanup(patch.stop)

    def remote_options(self):
        return ("--remote", "linux-host", "--remote-root", str(self.host_checkout))

    def test_the_probes_of_a_timing_step_run_on_the_host_too(self):
        culprit, head = self.commits_with_a_culprit()
        with mock.patch.dict(os.environ, {"FAKE_STEP": "tm"}):
            self.assertEqual(self.run_main(head, "--since", self.shas[0], *self.remote_options()), 1)
        record = self.record(head)
        self.assertEqual(record["culprit"]["sha"], culprit)
        self.assertEqual(record["load"]["host"], "linux-host", "the host's load judges the step")
        runs = self.runs()
        # The first run (it stops at the step on the host, so nothing runs here) and the rerun are not probes.
        probes = runs[2:]
        self.assertGreaterEqual(len(probes), 2)
        self.assertTrue(all("remote-gate/tree" in directory and arguments == "only=tm"
                            for directory, arguments in probes), probes)
        self.assertEqual(self.pauses, [])

    def test_a_rerun_that_fails_on_a_host_that_is_still_busy_is_inconclusive(self):
        self.stay_busy()
        record = self.flaky_record_under_load()
        self.assertIn("inconclusive", record["note"])
        self.assertNotIn("culprit", record)
        self.assertNotIn("candidates", record)
        self.assertEqual(sum(self.pauses), post_merge_hosts.LOAD_WAIT_BUDGET_SECONDS)
        self.assertEqual(record["load_after_rerun"]["average"], 80.0)

    def test_probes_that_fail_on_a_host_that_stays_busy_are_skipped_and_the_waiting_is_bounded_in_all(self):
        self.stay_busy()
        for name in ("two", "three", "four"):
            self.add(name, {name: "x"})
        head = self.add("five", {"e": "5"})
        self.noise.write_text("")
        # The record of the run, then the load at the failure (busy), the first wait and the end of
        # the rerun (quiet); every later reading, the probes', is busy.
        readings = iter([QUIET, BUSY, QUIET, QUIET])
        record = self.probe_record(head, lambda: next(readings, BUSY))
        self.assertNotIn("culprit", record, "a probe on a busy host blames nobody")
        self.assertEqual(len(record["candidates"]), 4)
        self.assertEqual(sum(self.pauses), post_merge_hosts.LOAD_WAIT_BUDGET_SECONDS, "one budget for all the probes")

    def test_a_probe_that_passes_on_a_busy_host_still_narrows_the_range(self):
        self.add("two", {"b": "2"})
        self.add("slows it down", {"broken": "x"})
        self.add("four", {"c": "3"})
        head = self.add("five", {"d": "4"})
        # The record of the run, the load at the failure and the end of the rerun are quiet (the
        # step fails because of `broken`); every reading of the probes is busy.
        readings = iter([QUIET, QUIET, QUIET])
        record = self.probe_record(head, lambda: next(readings, BUSY))
        self.assertNotIn("culprit", record)
        self.assertEqual([c["subject"] for c in record["candidates"]], ["slows it down", "four", "five"],
                         "the pass of the older commit on a busy host counts as good")

    def test_a_probe_that_fails_after_the_host_turned_busy_is_skipped(self):
        self.add("two", {"b": "2"})
        self.add("slows it down", {"broken": "x"})
        head = self.add("four", {"c": "3"})
        # Quiet up to the first probe's wait, then busy on every odd reading: after a failed probe.
        calls = []

        def local_load():
            calls.append(1)
            return QUIET if len(calls) <= 4 or len(calls) % 2 == 0 else BUSY

        record = self.probe_record(head, local_load)
        self.assertNotIn("culprit", record)
        self.assertEqual([c["subject"] for c in record["candidates"]], ["slows it down", "four"])

    def test_a_host_that_cannot_say_its_load_is_rerun_at_once(self):
        self.once.write_text("")  # the step fails once on the host, whatever its load
        with mock.patch.object(post_merge_hosts, "remote_load", lambda remote: None):
            record = self.flaky_record_under_load(*self.remote_options(), on_host=False)
        self.assertEqual(self.pauses, [], "an unreadable load is not waited for")
        self.assertNotIn("load", record)

    def test_a_rerun_that_fails_on_a_busy_remote_host_is_inconclusive(self):
        self.stay_busy()
        record = self.flaky_record_under_load(*self.remote_options(), on_host=True)
        self.assertIn("inconclusive", record["note"])
        self.assertEqual(record["load_after_rerun"], {"host": "linux-host", "average": 80.0, "cpus": 32})
        self.assertNotIn("culprit", record)
        self.assertEqual(sum(self.pauses), post_merge_hosts.LOAD_WAIT_BUDGET_SECONDS)

    def test_asking_a_silent_host_for_its_load_times_out(self):
        remote = post_merge_hosts.Remote("linux-host", str(self.host_checkout))
        with mock.patch.object(post_merge_hosts.subprocess, "run",
                               side_effect=subprocess.TimeoutExpired("ssh", 1)) as run:
            self.assertIsNone(REAL_REMOTE_LOAD(remote))
        self.assertEqual(run.call_args.kwargs["timeout"], post_merge_hosts.LOAD_PROBE_TIMEOUT_SECONDS)


if __name__ == "__main__":
    unittest.main()
