"""The two halves of a post-merge run: they run at the same time, stop cleanly, and a step that exits
with a code the gate also uses for "could not run" is still a failed step (scripts/post_merge_gate.py)."""

import os
import subprocess
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

import post_merge_gate
import post_merge_hosts
from test_post_merge_fixtures import RemoteHostCase


class SplitRunTests(RemoteHostCase):
    def test_the_two_halves_run_at_the_same_time(self):
        # Each half waits, in the stand-in gate, until the other has started: a run one after the
        # other would time out and fail the step.
        barrier = self.root.parent / "barrier"
        barrier.mkdir()
        head = self.add("two", {"b": "2"})
        with mock.patch.dict(os.environ, {"FAKE_BARRIER": str(barrier)}):
            self.assertEqual(self.run_remote(head), 0)
        self.assertEqual(self.record(head)["result"], "pass")
        self.assertEqual(len(list(barrier.iterdir())), 2)

    def test_the_log_holds_each_halves_lines_whole_and_the_host_output_after_the_run(self):
        self.assertEqual(self.run_remote(self.shas[0]), 0)
        head = self.add("breaks the engine step", {"broken": "x"})
        with mock.patch.dict(os.environ, {"FAKE_STEP": "eng"}):
            self.assertEqual(self.run_remote(head), 1)
        log = (self.health / f"{head}.log").read_text()
        self.assertIn("gate: output of linux-host:\nremote_gate: ", log)
        self.assertIn("check_all: FAILED step eng", log)
        self.assertEqual(list(self.health.glob("*.remote")), [])

    def failing_step_that_exits(self, step, number):
        """A run of a commit that breaks `step`, which exits with `number`: the exit code and the record."""
        self.assertEqual(self.run_remote(self.shas[0]), 0)
        head = self.add("breaks a step", {"broken": "x"})
        with mock.patch.dict(os.environ, {"FAKE_STEP": step, "FAKE_STEP_EXIT": str(number)}):
            return self.run_remote(head), self.record(head)

    def test_a_step_of_the_host_that_exits_2_is_a_failed_step_not_a_gate_that_could_not_run(self):
        code, record = self.failing_step_that_exits("eng", 2)
        self.assertEqual((code, record["result"], record["failed_step"]), (1, "fail", "eng"))

    def test_a_step_here_that_exits_2_is_a_failed_step_and_does_not_outrank_the_hosts_failure(self):
        code, record = self.failing_step_that_exits("sw", 2)
        self.assertEqual((code, record["result"], record["failed_step"]), (1, "fail", "sw"))
        self.assertEqual(self.record(record["sha"])["exit_code"], 1)

    def test_a_code_the_gate_uses_for_could_not_run_still_means_that_when_no_step_failed(self):
        for code in (2, 75, 78, 255):
            self.assertEqual(post_merge_gate.step_exit(code, False), code)
            self.assertEqual(post_merge_gate.step_exit(code, True), 1)
        self.assertEqual(post_merge_gate.step_exit(1, True), 1)
        self.assertEqual(post_merge_gate.step_exit(0, True), 0)

    def test_a_log_left_by_a_killed_run_is_not_part_of_the_next_one(self):
        head = self.add("two", {"b": "2"})
        self.health.mkdir(parents=True, exist_ok=True)
        (self.health / f"{head}.log.remote").write_text("STALE from a killed run\n")
        self.assertEqual(self.run_remote(head), 0)
        self.assertNotIn("STALE", (self.health / f"{head}.log").read_text())
        self.assertFalse((self.health / f"{head}.log.remote").exists())

    def test_the_host_section_ends_with_a_newline_and_a_log_that_cannot_be_written_hides_nothing(self):
        directory = Path(tempfile.mkdtemp())
        log, remote_log = directory / "run.log", directory / "run.log.remote"
        remote_log.write_bytes(b"no newline at the end")
        post_merge_gate.append_remote_output(log, remote_log, "linux-host")
        self.assertEqual(log.read_bytes(), b"gate: output of linux-host:\nno newline at the end\n")
        self.assertFalse(remote_log.exists())
        post_merge_gate.append_remote_output(log, directory / "missing", "linux-host")  # no exception
        post_merge_gate.append_remote_output(directory / "no" / "such" / "log", remote_log, "linux-host")

    def test_a_run_that_ignores_sigterm_is_killed_with_its_group_and_waiting_for_it_cannot_hang(self):
        directory = Path(tempfile.mkdtemp())
        child = directory / "child.pid"
        script = f"trap '' TERM; sleep 60 & echo $! > {child}; wait"
        process = post_merge_gate.start_process(["bash", "-c", script], directory, dict(os.environ),
                                                subprocess.DEVNULL)
        while not child.exists() or not child.read_text().strip():
            time.sleep(0.05)
        time.sleep(0.2)  # the trap is set before the child starts
        started = time.monotonic()
        with mock.patch.object(post_merge_gate, "STOP_GRACE_SECONDS", 1):
            post_merge_gate.stop_process(process)
        self.assertLess(time.monotonic() - started, 20)
        for _ in range(100):
            try:
                os.kill(int(child.read_text()), 0)
            except ProcessLookupError:
                break
            time.sleep(0.05)
        else:
            self.fail("the child of the stopped run survived")

    def test_a_second_interrupt_while_one_half_stops_still_stops_the_other(self):
        stopped = []

        def stop(process):
            stopped.append(process)
            if process == "first":
                raise KeyboardInterrupt

        with mock.patch.object(post_merge_gate, "stop_process", stop), self.assertRaises(KeyboardInterrupt):
            post_merge_gate.stop_processes(["first", None, "second"])
        self.assertEqual(stopped, ["first", "second"])

    def test_a_host_that_cannot_run_the_gate_stops_the_local_half_instead_of_waiting_for_it(self):
        busy = 'if [[ "$*" == *"--sha"* ]]; then exit 75; fi\nexec ' + str(self.fake_ssh) + ' "$@"'
        started = time.monotonic()
        busy_ssh = self.root.parent / "busy_ssh"
        busy_ssh.write_text("#!/bin/bash\n" + busy + "\n")
        busy_ssh.chmod(0o755)
        environment = {post_merge_hosts.SSH_ENV: str(busy_ssh), "FAKE_SLOW_LOCAL": "60"}
        with mock.patch.dict(os.environ, environment):
            self.assertEqual(self.run_remote(self.shas[0]), 2)
        self.assertLess(time.monotonic() - started, 30)
        self.assertFalse(list(self.health.glob("*.json")))



if __name__ == "__main__":
    unittest.main()
