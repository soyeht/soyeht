import os
import subprocess
import unittest
from unittest import mock

import post_merge_gate
import post_merge_hosts
from test_post_merge_fixtures import RemoteHostCase, git


class RemoteGateTests(RemoteHostCase):
    """--remote: engine, desktop and e2e steps through scripts/remote_gate.sh, the rest here."""

    def test_remote_steps_run_on_the_host_first_and_the_rest_here(self):
        head = self.add("two", {"b": "2"})
        self.assertEqual(self.run_remote(head), 0)
        record = self.record(head)
        self.assertEqual(record["result"], "pass")
        self.assertEqual(record["remote"], "linux-host")
        (remote_directory, remote_arguments), (local_directory, local_arguments) = self.runs()
        self.assertIn("remote-gate/tree", remote_directory)
        self.assertEqual(remote_arguments.split()[0], "only=eng,snap")
        self.assertNotIn("remote-gate", local_directory)
        self.assertEqual(local_arguments.split()[0], "only=unit,sw,tcc")
        self.assertEqual(git(self.host_checkout / ".cache/remote-gate/tree", "rev-parse", "HEAD"), head)
        self.assertEqual(self.ssh_log.read_text().split(), ["linux-host", "linux-host"])  # run, timings

    def test_the_base_goes_to_both_sides(self):
        head = self.add("two", {"b": "2"})
        self.assertEqual(self.run_remote(head), 0)
        parent = self.shas[0]
        self.assertTrue(all(f"base={parent}" in arguments for _, arguments in self.runs()))

    def test_a_remote_failure_names_the_step_and_the_culprit_without_running_the_rest_here(self):
        self.assertEqual(self.run_remote(self.shas[0]), 0)
        self.add("two", {"b": "2"})
        culprit = self.add("breaks the engine step", {"broken": "x"})
        head = self.add("four", {"c": "3"})
        self.trace.unlink()
        with mock.patch.dict(os.environ, {"FAKE_STEP": "eng"}):
            self.assertEqual(self.run_remote(head), 1)
        record = self.record(head)
        self.assertEqual(record["failed_step"], "eng")
        self.assertEqual(record["culprit"]["sha"], culprit)
        self.assertEqual([step["id"] for step in record["steps"]], ["eng"])
        self.assertTrue(all("remote-gate" in directory for directory, _ in self.runs()))

    def test_a_local_failure_is_found_after_the_remote_steps_pass(self):
        self.assertEqual(self.run_remote(self.shas[0]), 0)
        head = self.add("breaks the Swift step", {"broken": "x"})
        with mock.patch.dict(os.environ, {"FAKE_STEP": "sw"}):
            self.assertEqual(self.run_remote(head), 1)
        record = self.record(head)
        self.assertEqual(record["failed_step"], "sw")
        self.assertEqual(record["culprit"]["sha"], head)

    def test_an_unreachable_host_records_nothing(self):
        broken_ssh = self.root.parent / "broken_ssh"
        broken_ssh.write_text("#!/bin/bash\nexit 255\n")
        broken_ssh.chmod(0o755)
        with mock.patch.dict(os.environ, {post_merge_hosts.SSH_ENV: str(broken_ssh)}):
            self.assertEqual(self.run_remote(self.shas[0]), 2)
        self.assertFalse(list(self.health.glob("*.json")))

    def test_a_root_with_a_tilde_is_the_hosts_home(self):
        with mock.patch.dict(os.environ, {"HOME": str(self.root.parent)}):
            self.assertEqual(self.run_remote(self.shas[0], root="~/host"), 0)
        self.assertEqual(post_merge_hosts.Remote("h", "~").remote_path(), '"$HOME"\'\'')
        self.assertEqual(post_merge_hosts.Remote("h", "/a b").remote_path(), "'/a b'")

    def wrapped_git(self):
        """A git that logs each use, and the log."""
        real = subprocess.run(["which", "git"], capture_output=True, text=True).stdout.strip()
        log = self.root.parent / "git-used.txt"
        wrapper = self.root.parent / "wrapped-git"
        wrapper.write_text(f'#!/bin/bash\necho "$@" >> {log}\nexec {real} "$@"\n')
        wrapper.chmod(0o755)
        return wrapper, log

    def test_the_host_reads_its_git_from_the_environment_or_the_env_file(self):
        wrapper, log = self.wrapped_git()
        head = self.shas[0]
        with mock.patch.dict(os.environ, {"SOYEHT_GATE_GIT": str(wrapper)}):
            self.assertEqual(self.script_run("--sha", head, "--steps", "unit").returncode, 0)
        self.assertIn("worktree", log.read_text())
        log.unlink()
        state = self.host_checkout / ".cache/remote-gate"
        (state / "gate.env").write_text(f"SOYEHT_GATE_GIT={wrapper}\n")
        self.assertEqual(self.script_run("--sha", head, "--steps", "unit").returncode, 0)
        self.assertIn("checkout", log.read_text())

    def test_a_failing_env_file_is_a_host_problem(self):
        env_file = self.host_checkout / ".cache/remote-gate/gate.env"
        env_file.parent.mkdir(parents=True, exist_ok=True)
        for body in ("false\nA=1\n", "cd /nonexistent/dir\n", "A=$((\n"):
            env_file.write_text(body)
            result = self.script_run("--sha", self.shas[0], "--steps", "unit")
            self.assertEqual(result.returncode, 2, body)
            self.assertIn("gate.env failed", result.stderr)

    def test_an_env_file_others_can_write_is_refused(self):
        env_file = self.host_checkout / ".cache/remote-gate/gate.env"
        env_file.parent.mkdir(parents=True, exist_ok=True)
        env_file.write_text("A=1\n")
        for mode in (0o664, 0o646):
            env_file.chmod(mode)
            result = self.script_run("--sha", self.shas[0], "--steps", "unit")
            self.assertEqual(result.returncode, 2, oct(mode))
            self.assertIn("not writable by others", result.stderr)
        env_file.chmod(0o644)
        self.assertEqual(self.script_run("--sha", self.shas[0], "--steps", "unit").returncode, 0)

    def test_a_host_without_a_usable_git_is_a_host_problem(self):
        with mock.patch.dict(os.environ, {"SOYEHT_GATE_GIT": "/nonexistent/git"}):
            result = self.script_run("--sha", self.shas[0], "--steps", "unit")
        self.assertEqual(result.returncode, 2)
        self.assertIn("SOYEHT_GATE_GIT", result.stderr)

    def test_a_host_that_is_busy_on_the_rerun_blames_no_commit(self):
        self.assertEqual(self.run_remote(self.shas[0]), 0)
        self.add("two", {"b": "2"})
        self.add("breaks the engine step", {"broken": "x"})
        head = self.add("four", {"c": "3"})
        seen = self.root.parent / "gate-calls"
        flaky_ssh = self.root.parent / "flaky_ssh"
        flaky_ssh.write_text(f"""#!/bin/bash
# The first gate call goes through; every later one finds the host's worktree busy.
if [[ "$*" == *"--sha"* ]]; then
    [[ -e {seen} ]] && exit 75
    touch {seen}
fi
exec {self.fake_ssh} "$@"
""")
        flaky_ssh.chmod(0o755)
        with mock.patch.dict(os.environ, {post_merge_hosts.SSH_ENV: str(flaky_ssh), "FAKE_STEP": "eng"}):
            self.assertEqual(self.run_remote(head), 1)
        record = self.record(head)
        self.assertEqual(record["result"], "fail")
        self.assertEqual(record["failed_step"], "eng")
        self.assertNotIn("culprit", record)
        self.assertNotIn("candidates", record)
        self.assertIn("rerun", record["note"])

    def test_a_busy_host_worktree_records_nothing_and_blames_no_commit(self):
        busy_ssh = self.root.parent / "busy_ssh"
        busy_ssh.write_text("#!/bin/bash\nexit 75\n")
        busy_ssh.chmod(0o755)
        with mock.patch.dict(os.environ, {post_merge_hosts.SSH_ENV: str(busy_ssh)}):
            self.assertEqual(self.run_remote(self.shas[0]), post_merge_gate.INFRASTRUCTURE_EXIT)
            self.add("two", {"b": "2"})
            head = self.add("three", {"c": "3"})
            self.assertEqual(self.run_remote(head), post_merge_gate.INFRASTRUCTURE_EXIT)
        self.assertFalse(list(self.health.glob("*.json")))

    def test_a_host_without_the_checkout_or_the_script_records_nothing(self):
        self.assertEqual(self.run_remote(self.shas[0], root=self.root.parent / "missing"), 2)
        (self.host_checkout / "scripts/remote_gate.sh").unlink()
        self.assertEqual(self.run_remote(self.shas[0]), 2)
        self.assertFalse(list(self.health.glob("*.json")))

    def test_remote_needs_a_root(self):
        with self.assertRaises(SystemExit):
            self.run_main(self.shas[0], "--remote", "linux-host")

    def test_the_persistent_worktree_keeps_ignored_files_and_drops_untracked_ones(self):
        self.assertEqual(self.run_remote(self.shas[0]), 0)
        tree = self.host_checkout / ".cache/remote-gate/tree"
        (tree / "stray").write_text("x")
        head = self.add("two", {"b": "2"})
        self.assertEqual(self.run_remote(head), 0)
        self.assertFalse((tree / "stray").exists())
        self.assertEqual(git(tree, "rev-parse", "HEAD"), head)

    def test_the_script_refuses_a_bad_request(self):
        script = self.host_checkout / "scripts/remote_gate.sh"
        result = subprocess.run(["bash", str(script), "--sha", "main", "--steps", "eng"],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 64)
        result = subprocess.run(["bash", str(script), "--bogus"], capture_output=True, text=True)
        self.assertEqual(result.returncode, 64)

    def test_an_unknown_commit_is_a_host_problem(self):
        script = self.host_checkout / "scripts/remote_gate.sh"
        result = subprocess.run(["bash", str(script), "--sha", "0" * 40, "--steps", "eng"],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("cannot fetch", result.stderr)

    def script_run(self, *arguments):
        script = self.host_checkout / "scripts/remote_gate.sh"
        return subprocess.run(["bash", str(script), *arguments], capture_output=True, text=True)

    def test_a_slot_has_its_own_worktree_and_timings(self):
        head = self.shas[0]
        result = self.script_run("--sha", head, "--steps", "unit", "--slot", "slot1")
        self.assertEqual(result.returncode, 0, result.stderr)
        state = self.host_checkout / ".cache/remote-gate"
        self.assertEqual(git(state / "tree-slot1", "rev-parse", "HEAD"), head)
        self.assertFalse((state / "tree").exists())
        self.assertTrue((state / "timings-slot1.json").is_file())
        self.assertIn("remote_gate: 1.0s 0 unit", result.stdout)
        self.assertIn(f"remote_gate: gate-timings {state / 'timings-slot1.json'}", result.stdout)
        self.assertEqual(result.stdout.splitlines()[-1], "remote_gate: exit 0")
        printed = self.script_run("--print-timings", "--slot", "slot1")
        self.assertIn('"unit"', printed.stdout)

    def test_the_exit_line_carries_the_gates_code(self):
        broken = self.add("broken", {"broken": "x"})
        with mock.patch.dict(os.environ, {"FAKE_STEP": "unit"}):
            result = self.script_run("--sha", broken, "--steps", "unit")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout.splitlines()[-1], "remote_gate: exit 1")

    def test_the_host_reports_its_load_and_processor_count(self):
        result = self.script_run("--print-load")
        self.assertEqual(result.returncode, 0, result.stderr)
        average, cpus = result.stdout.split()
        self.assertGreaterEqual(float(average), 0.0)
        self.assertGreaterEqual(int(cpus), 1)
        load = post_merge_hosts.remote_load(post_merge_hosts.Remote("linux-host", str(self.host_checkout)))
        self.assertEqual((load.host, load.cpus >= 1), ("linux-host", True))

    def test_a_bad_slot_name_is_refused(self):
        result = self.script_run("--sha", self.shas[0], "--steps", "unit", "--slot", "../x")
        self.assertEqual(result.returncode, 64)


# A stand-in with the three kinds of step the load rules tell apart: `unit` (no latency limit), `tm`
# (area `timing`) and the Rust editor step (area `engine`); with a host, `tm` and the editor step run
# there and `unit` here. A step named by FAKE_STEP fails while the file FAKE_NOISE exists (a loaded


if __name__ == "__main__":
    unittest.main()
