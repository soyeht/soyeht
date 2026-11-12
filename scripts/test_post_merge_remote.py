import contextlib
import io
import json
import os
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest import mock

import gate_remote
import post_merge_gate
import post_merge_hosts
from test_post_merge_fixtures import RemoteHostCase, git, isolate_environment


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

    def test_the_commits_title_and_body_reach_the_local_half_as_the_pull_request_event(self):
        # A squash merge keeps the pull request's text in the commit; a waived drop reads it there.
        head = self.add("Drop a step\n\nVerification change: dropped on purpose", {"b": "2"})
        events = self.root.parent / "events"
        events.mkdir()
        with mock.patch.dict(os.environ, {"FAKE_EVENTS": str(events)}):
            self.assertEqual(self.run_remote(head), 0)
        pull_request = json.loads((events / f"{head}.json").read_text())["pull_request"]
        self.assertEqual(pull_request["title"], "Drop a step")
        self.assertIn("Verification change: dropped on purpose", pull_request["body"])

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

    def test_the_seed_of_the_long_fuzz_run_goes_to_the_host_in_the_request(self):
        # The host does not inherit this process's environment, so the request carries the seed and
        # the host script puts it in the gate's environment.
        remote = post_merge_hosts.Remote("linux-host", str(self.host_checkout))
        arguments = post_merge_gate.remote_gate_arguments
        with mock.patch.dict(os.environ, {"SOYEHT_FUZZ_SEED": "4242"}):
            self.assertIn(" --fuzz-seed 4242", arguments(remote, self.shas[0], ["eng"], None, None, ""))
        with mock.patch.dict(os.environ):
            os.environ.pop("SOYEHT_FUZZ_SEED", None)
            self.assertNotIn("--fuzz-seed", arguments(remote, self.shas[0], ["eng"], None, None, ""))
            seeds = self.root.parent / "seeds.txt"
            environment = {**os.environ, "FAKE_SEEDS": str(seeds)}
            subprocess.run(["bash", str(self.host_checkout / "scripts/remote_gate.sh"), "--sha", self.shas[0],
                            "--steps", "eng", "--fuzz-seed", "777"], capture_output=True, cwd=self.host_checkout,
                           env=environment, check=True)
            self.assertEqual(seeds.read_text().split()[1], "777")


# A stand-in with the steps the pre-merge split tells apart, which lists them as the tier would
# (`--only` names steps of any tier) and records each step it runs, in the timings file, with a
# trace of the tier, the steps, the base and the engine crates it was given.
FAKE_TIER_GATE = r"""#!/bin/bash
tier=full; only=""; base=""; list=0; event=""
while (($#)); do case "$1" in --tier) tier="$2"; shift 2;; --only) only="$2"; shift 2;; --base) base="$2"; shift 2;; --event-file) event="$2"; shift 2;; --list) list=1; shift;; *) shift;; esac; done
rows='always both unit
engine both eng
engine both engine:build
engine fast engine:test-changed
engine full engine:test
swift both sw
e2e both snap'
selected=""
while read -r area tiers id; do
    if [[ -n "$only" ]]; then [[ ",$only," == *",$id,"* ]] || continue
    elif [[ "$tiers" != both && "$tiers" != "$tier" ]]; then continue; fi
    selected+="$area $tiers $id"$'\n'
done <<< "$rows"
[[ -z "${FAKE_EMPTY_PLAN:-}" ]] || selected=""
if ((list)); then printf '%s' "$selected"; exit 0; fi
echo "$PWD tier=$tier only=$only base=$base event=$event engine=${SOYEHT_GATE_ENGINE_TEST_ARGS:-}" >> "${FAKE_TRACE:?}"
[[ -z "${FAKE_WINDOWS_SKIP:-}" ]] || echo "check_all: skipped the Windows lint; run: rustup target add x86_64-pc-windows-msvc"
steps=""; status=0
while read -r area tiers id; do
    [[ -n "$id" ]] || continue
    status=0; [[ "$id" == "${FAKE_STEP:-}" ]] && status=1
    steps+="${steps:+,}{\"id\":\"$id\",\"area\":\"$area\",\"seconds\":2.0,\"status\":$status}"
    if ((status)); then echo "check_all: FAILED step $id (area $area, exit 1)" >&2; break; fi
done <<< "$selected"
printf '{"latest":{"steps":[%s]}}' "$steps" > "${SOYEHT_GATE_TIMINGS:?}"
exit "$status"
"""


# What `gate_plan.py --format shell` prints: the plan's text goes to stderr, the assignments to stdout.
FAKE_PLAN = """import sys
print("gate plan", file=sys.stderr)
print("GATE_AREAS='engine'")
print("GATE_ENGINE_TEST_ARGS='-p soyeht-store'")
"""


class PullRequestGateTests(RemoteHostCase):
    """scripts/gate_remote.py: the pre-merge gate, split between the host and this machine, for a
    head that is on no remote."""

    FILES = {**RemoteHostCase.FILES, "scripts/check_all.sh": FAKE_TIER_GATE, "scripts/gate_plan.py": FAKE_PLAN}

    def setUp(self):
        super().setUp()
        # The host reaches nothing but what is sent to it, as a host fed by a mirror that lacks the head.
        git(self.host_checkout, "remote", "set-url", "origin", str(self.root.parent / "nowhere"))
        self.head = self.add("the pull request", {"b": "2"})
        self.timings = self.root.parent / "gate-timings.json"
        environment = mock.patch.dict(os.environ, {"SOYEHT_GATE_TIMINGS": str(self.timings)})
        environment.start()
        self.addCleanup(environment.stop)
        self.remote = post_merge_hosts.Remote("linux-host", str(self.host_checkout))

    def split_gate(self, *extra, tier="fast"):
        """gate_remote.run in the synthetic repository: the exit code and the output."""
        arguments = gate_remote.parse(["--tier", tier, "--base", self.shas[0], "--remote", "linux-host",
                                       "--remote-root", str(self.host_checkout), "--plan", "python3 scripts/gate_plan.py",
                                       *extra])
        output = self.root.parent / "gate-output.txt"
        with captured_stdout(output):
            code = gate_remote.run(arguments, self.root)
        return code, output.read_text()

    def ssh_script(self, body):
        """Inside the context, ssh is this script."""
        path = self.root.parent / "scripted_ssh"
        path.write_text("#!/bin/bash\n" + body + "\n")
        path.chmod(0o755)
        return mock.patch.dict(os.environ, {post_merge_hosts.SSH_ENV: str(path)})

    def sides(self):
        """The arguments of the run on the host and of the run here, whichever started first."""
        runs = self.runs()
        remote = [arguments for directory, arguments in runs if "remote-gate" in directory]
        local = [arguments for directory, arguments in runs if "remote-gate" not in directory]
        return remote, local

    def host_state(self):
        return self.host_checkout / ".cache/remote-gate"

    def test_the_head_goes_to_the_host_in_a_bundle_and_the_halves_split(self):
        code, output = self.split_gate()
        self.assertEqual(code, 0, output)
        self.assertEqual(git(self.host_state() / "tree", "rev-parse", "HEAD"), self.head)
        (remote_arguments,), (local_arguments,) = self.sides()
        self.assertIn("only=eng,engine:build,engine:test-changed,snap", remote_arguments)
        self.assertIn("engine=-p soyeht-store", remote_arguments)
        self.assertIn("tier=fast only=unit,sw", local_arguments)
        self.assertIn(f"base={self.shas[0]}", local_arguments)
        # the bundle and the ref that carried the commit are gone from the host
        self.assertEqual(list(self.host_state().glob("*.bundle")), [])
        self.assertEqual(git(self.host_checkout, "for-each-ref", "refs/remote-gate"), "")
        self.assertIn("gate_remote: passed", output)

    def test_both_halves_report_their_times_in_the_one_timings_file(self):
        self.assertEqual(self.split_gate()[0], 0)
        latest = json.loads(self.timings.read_text())["latest"]
        self.assertEqual(latest["tier"], "fast+remote:linux-host")
        self.assertEqual(latest["exit_code"], 0)
        hosts = {step["id"]: step.get("host") for step in latest["steps"]}
        self.assertEqual(hosts, {"eng": "linux-host", "engine:build": "linux-host",
                                 "engine:test-changed": "linux-host", "snap": "linux-host",
                                 "unit": None, "sw": None})

    def test_the_timings_default_to_the_gated_worktree_not_the_scripts_checkout(self):
        with mock.patch.dict(os.environ):
            os.environ.pop("SOYEHT_GATE_TIMINGS")
            self.assertEqual(self.split_gate()[0], 0)
        self.assertTrue((self.root / ".cache/gate-timings.json").is_file())

    def test_a_host_without_the_base_gets_the_whole_history_and_one_with_it_an_increment(self):
        git(self.host_checkout, "update-ref", "-d", "refs/remotes/origin/main")
        self.add("another", {"c": "3"})
        scratch = self.root.parent / "scratch-bundles"
        scratch.mkdir()
        requires = []
        for base in (self.shas[0], self.head):  # the host has the first; the second only comes with the bundle
            path = post_merge_hosts.ship_commit(self.remote, self.root, base, scratch)
            requires.append("requires" in git(self.host_checkout, "bundle", "verify", str(self.host_checkout / path)))
            (self.host_checkout / path).unlink()
        self.assertEqual(requires, [True, False])
        self.assertEqual(list(scratch.iterdir()), [])

    def test_a_head_the_host_holds_is_not_sent(self):
        git(self.host_checkout, "remote", "set-url", "origin", str(self.root))
        self.assertIsNone(post_merge_hosts.ship_commit(self.remote, self.root, self.shas[0], self.root.parent))
        self.assertTrue(post_merge_hosts.host_has_commit(self.remote, self.head))

    def test_an_unreachable_host_is_a_clear_failure_and_nothing_runs(self):
        with self.ssh_script("exit 255"):
            code, output = self.split_gate()
        self.assertEqual(code, post_merge_gate.INFRASTRUCTURE_EXIT)
        self.assertIn("cannot ask linux-host", output)
        self.assertFalse(self.trace.exists())
        self.assertIn("FAILED (exit 2)", output)

    def test_a_remote_failure_fails_the_gate_while_the_local_half_still_reports(self):
        with mock.patch.dict(os.environ, {"FAKE_STEP": "engine:build"}):
            code, output = self.split_gate()
        self.assertEqual(code, 1)
        self.assertIn("check_all: FAILED step engine:build", output)
        self.assertEqual(len(self.runs()), 2)
        steps = json.loads(self.timings.read_text())["latest"]["steps"]
        self.assertEqual([step["id"] for step in steps if step["status"]], ["engine:build"])

    def test_a_local_failure_fails_the_gate_after_the_remote_steps_pass(self):
        with mock.patch.dict(os.environ, {"FAKE_STEP": "sw"}):
            code, output = self.split_gate()
        self.assertEqual(code, 1)
        self.assertIn("check_all: FAILED step sw", output)

    def test_sequential_stops_at_a_remote_failure_without_running_the_rest_here(self):
        with mock.patch.dict(os.environ, {"FAKE_STEP": "eng"}):
            code, _ = self.split_gate("--sequential")
        self.assertEqual(code, 1)
        self.assertEqual(len(self.runs()), 1)
        self.assertIn("remote-gate/tree", self.runs()[0][0])

    def test_a_busy_host_worktree_is_an_infrastructure_failure_and_its_bundle_is_removed(self):
        with self.ssh_script(f'if [[ "$*" == *"--sha"* ]]; then exit 75; fi\nexec {self.fake_ssh} "$@"'):
            code, output = self.split_gate()
        self.assertEqual(code, post_merge_gate.INFRASTRUCTURE_EXIT)
        self.assertIn("is busy with another run", output)
        self.assertEqual(list(self.host_state().glob("*.bundle")), [])

    def test_a_slot_gives_the_run_its_own_host_worktree(self):
        self.assertEqual(self.split_gate("--remote-slot", "slot1")[0], 0)
        self.assertEqual(git(self.host_state() / "tree-slot1", "rev-parse", "HEAD"), self.head)
        self.assertFalse((self.host_state() / "tree").exists())

    def test_a_rerun_of_one_engine_test_builds_first_and_has_no_plan(self):
        code, _ = self.split_gate("--only", "engine:test")
        self.assertEqual(code, 0)
        ((remote_arguments,), _) = self.sides()
        self.assertIn("only=engine:test,engine:build", remote_arguments)
        self.assertIn("engine= ", remote_arguments + " ")

    def test_the_full_tier_sends_no_engine_selection(self):
        self.assertEqual(self.split_gate(tier="full")[0], 0)
        self.assertIn("engine= ", self.sides()[0][0] + " ")

    def test_the_event_file_reaches_the_local_half_only(self):
        event = self.root.parent / "event.json"
        event.write_text("{}")
        self.assertEqual(self.split_gate("--event-file", str(event))[0], 0)
        (remote_arguments,), (local_arguments,) = self.sides()
        self.assertIn(f"event={event}", local_arguments)
        self.assertIn("event= ", remote_arguments + " ")

    def test_a_gate_wrapper_slot_in_the_environment_does_not_move_the_hosts_worktree(self):
        # A gate that runs through gate_remote.py with a slot hands SOYEHT_REMOTE_SLOT to the local
        # half, which runs these tests: the stand-in host must still use its default `tree`.
        wrapper = {gate_remote.SLOT_ENV: "slot1", gate_remote.HOST_ENV: "other", gate_remote.ROOT_ENV: "/nowhere",
                   "SOYEHT_REMOTE_GATE_DIR": "/nowhere", "SOYEHT_GATE_ENV_FILE": "/nowhere"}
        with mock.patch.dict(os.environ, wrapper):
            isolate_environment()
            self.assertEqual({name for name in wrapper if name in os.environ}, set())

    def test_it_refuses_to_run_without_a_host(self):
        with mock.patch.dict(os.environ, {}, clear=False):
            for name in (gate_remote.HOST_ENV, gate_remote.ROOT_ENV):
                os.environ.pop(name, None)
            with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
                gate_remote.parse(["--tier", "fast"])

    def test_an_empty_plan_is_not_a_pass(self):
        with mock.patch.dict(os.environ, {"FAKE_EMPTY_PLAN": "1"}):
            code, output = self.split_gate()
        self.assertEqual(code, post_merge_gate.REMOTE_UNSUPPORTED_EXIT)
        self.assertIn("selects no step", output)
        self.assertFalse(self.trace.exists())

    def test_a_misspelled_only_step_is_refused_before_anything_runs(self):
        code, output = self.split_gate("--only", "engine:build,engine:buidl")
        self.assertEqual(code, post_merge_gate.REMOTE_UNSUPPORTED_EXIT)
        self.assertIn("engine:buidl", output)
        self.assertFalse(self.trace.exists())

    def test_a_dirty_worktree_is_refused_because_the_halves_would_gate_different_trees(self):
        (self.root / "b").write_text("changed after the commit")
        code, _ = self.split_gate()
        self.assertEqual(code, post_merge_gate.INFRASTRUCTURE_EXIT)
        self.assertFalse(self.trace.exists())
        (self.root / "untracked").write_text("not in the commit, not read by the gate")
        git(self.root, "checkout", "--", "b")
        self.assertEqual(self.split_gate()[0], 0)

    def test_a_host_that_skipped_the_windows_lint_is_warned_about_loudly(self):
        with mock.patch.dict(os.environ, {"FAKE_WINDOWS_SKIP": "1"}):
            code, output = self.split_gate()
        self.assertEqual(code, 0)
        self.assertIn("WARNING: linux-host skipped the Windows lint", output)
        self.assertNotIn("WARNING", self.split_gate()[1])

    def test_a_gate_that_could_not_run_outranks_a_step_that_failed(self):
        busy = 'if [[ "$*" == *"--sha"* ]]; then exit 75; fi\nexec ' + str(self.fake_ssh) + ' "$@"'
        with self.ssh_script(busy), mock.patch.dict(os.environ, {"FAKE_STEP": "sw"}):
            code, output = self.split_gate()
        self.assertEqual(code, post_merge_gate.INFRASTRUCTURE_EXIT)
        self.assertIn("check_all: FAILED step sw", output)  # the local half ran and failed too
        self.assertEqual(post_merge_gate.combine_codes(1, 1), 1)
        self.assertEqual(post_merge_gate.combine_codes(1, 78), 78)

    def test_ssh_dying_while_both_halves_run_records_no_remote_times(self):
        died = 'if [[ "$*" == *"--sha"* ]]; then exit 255; fi\nexec ' + str(self.fake_ssh) + ' "$@"'
        with self.ssh_script(died):
            code, output = self.split_gate()
        self.assertEqual(code, post_merge_gate.INFRASTRUCTURE_EXIT)
        self.assertIn("cannot reach linux-host over ssh", output)
        self.assertEqual(len(self.sides()[1]), 1)  # the local half still ran
        steps = json.loads(self.timings.read_text())["latest"]["steps"]
        self.assertEqual([step["id"] for step in steps], ["unit", "sw"])

    def test_an_interrupt_stops_the_host_run_and_removes_the_bundle(self):
        pid_file = self.root.parent / "ssh.pid"
        hung = f'if [[ "$*" == *"--sha"* ]]; then echo $$ > {pid_file}; exec sleep 60; fi\nexec {self.fake_ssh} "$@"'
        def interrupted(*_):
            while not pid_file.exists():  # the host run has started
                time.sleep(0.05)
            raise KeyboardInterrupt

        with self.ssh_script(hung), mock.patch.object(post_merge_gate, "run_local_steps", side_effect=interrupted):
            with self.assertRaises(KeyboardInterrupt):
                self.split_gate()
        with self.assertRaises(ProcessLookupError):
            os.kill(int(pid_file.read_text()), 0)
        self.assertEqual(list(self.host_state().glob("*.bundle")), [])

    def test_a_bundle_that_arrived_in_part_is_removed_when_the_upload_fails(self):
        failing = ('"' + str(self.fake_ssh) + '" "$@"\nstatus=$?\n'
                   '[[ "$*" == *"cat >"* ]] && exit 1\nexit $status')
        with self.ssh_script(failing):
            code, output = self.split_gate()
        self.assertEqual(code, post_merge_gate.INFRASTRUCTURE_EXIT)
        self.assertIn("cannot send the bundle", output)
        self.assertEqual(list(self.host_state().glob("*.bundle")), [])

    def test_a_plan_that_cannot_be_read_tests_the_whole_workspace(self):
        arguments = gate_remote.parse(["--remote", "h", "--remote-root", "r", "--plan", "python3 -c 'import sys; sys.exit(3)'"])
        self.assertEqual(gate_remote.engine_test_args(arguments, self.root, self.shas[0]), "")
        arguments = gate_remote.parse(["--remote", "h", "--remote-root", "r", "--plan", "python3 scripts/gate_plan.py"])
        self.assertEqual(gate_remote.engine_test_args(arguments, self.root, self.shas[0]), "-p soyeht-store")

    def test_a_slot_name_the_host_would_refuse_is_refused_here(self):
        with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
            gate_remote.parse(["--remote", "h", "--remote-root", "r", "--remote-slot", "../x"])

    def test_the_host_script_only_touches_a_bundle_it_would_have_been_sent(self):
        script = self.host_checkout / "scripts/remote_gate.sh"

        def ask(bundle):
            return subprocess.run(["bash", str(script), "--sha", self.head, "--steps", "eng", "--bundle", bundle],
                                  capture_output=True, text=True, cwd=self.host_checkout).returncode

        for bad in ("/etc/passwd", "../gate-0123456789ab.bundle", "gate-xyz.bundle", "x/gate-0123456789ab.bundle"):
            self.assertEqual(ask(bad), 64, bad)
        self.assertEqual(ask(".cache/remote-gate/gate-0123456789ab-slot1.bundle"), 2)  # well named, not there

    def test_the_host_script_drops_the_refs_a_killed_run_left(self):
        git(self.host_checkout, "update-ref", f"refs/remote-gate/default-{self.shas[0]}", self.shas[0])
        git(self.host_checkout, "update-ref", f"refs/remote-gate/other-{self.shas[0]}", self.shas[0])
        script = self.host_checkout / "scripts/remote_gate.sh"
        subprocess.run(["bash", str(script), "--sha", self.shas[0], "--steps", "eng"], capture_output=True,
                       cwd=self.host_checkout, check=True)
        self.assertEqual(git(self.host_checkout, "for-each-ref", "--format=%(refname)", "refs/remote-gate").split(),
                         [f"refs/remote-gate/other-{self.shas[0]}"])

    def test_the_host_script_answers_has_and_refuses_bad_arguments(self):
        script = self.host_checkout / "scripts/remote_gate.sh"

        def ask(*arguments):
            return subprocess.run(["bash", str(script), *arguments], capture_output=True, text=True,
                                  cwd=self.host_checkout).returncode

        self.assertEqual(ask("--has", self.shas[0]), 0)
        self.assertEqual(ask("--has", self.head), 1)
        self.assertEqual(ask("--has", "main"), 64)
        self.assertEqual(ask("--sha", self.head, "--steps", "eng", "--engine-test-args", "-p a; rm -rf /"), 64)
        self.assertEqual(ask("--sha", self.head, "--steps", "eng", "--bundle", "gate-0123456789ab.bundle"), 2)
        for bad in ("abc", "-1", "1; rm -rf /", "12345678901234567890"):
            self.assertEqual(ask("--sha", self.head, "--steps", "eng", "--fuzz-seed", bad), 64, bad)


class BrokenSink:
    def write(self, data):
        raise BrokenPipeError

    def flush(self):
        raise BrokenPipeError


class PumpTests(unittest.TestCase):
    """The reader of the host run's output must never keep the gate waiting or ssh blocked."""

    def start(self, script, sink):
        process = post_merge_gate.start_remote(["bash", "-c", script], Path.cwd(), sink)
        # a hang is a failure, not a stuck test run
        watchdog = threading.Timer(30, lambda: os.killpg(process.pid, 9))
        watchdog.start()
        self.addCleanup(watchdog.cancel)
        return process

    def test_a_process_that_inherited_the_pipe_does_not_hold_the_gate_after_ssh_exits(self):
        started = time.monotonic()
        holder = Path(tempfile.mkdtemp()) / "holder.pid"
        with mock.patch.object(post_merge_gate, "DRAIN_SECONDS", 1):
            process = self.start(f"echo done; sleep 8 & echo $! > {holder}; exit 3", io.BytesIO())
            self.assertEqual(post_merge_gate.wait_process(process), 3)
        self.assertLess(time.monotonic() - started, 6)
        os.kill(int(holder.read_text()), 15)  # the holder is ours

    def test_a_sink_that_fails_is_dropped_and_the_output_is_still_drained_and_scanned(self):
        # More than a pipe holds (64 KB), then the line the scan looks for.
        script = "head -c 300000 /dev/zero | tr '\\0' x; echo; echo 'check_all: skipped the Windows lint; run: x'"
        process = self.start(script, BrokenSink())
        self.assertEqual(post_merge_gate.wait_process(process), 0)
        self.assertTrue(process.pump.windows_lint_skipped)
        self.assertIsNone(process.pump.sink)

    def test_the_lint_line_is_found_across_two_reads_and_a_good_sink_gets_everything(self):
        sink = io.BytesIO()
        process = self.start("printf 'check_all: skipped the Wi'; sleep 0.5; printf 'ndows lint; run: x\\n'", sink)
        self.assertEqual(post_merge_gate.wait_process(process), 0)
        self.assertTrue(process.pump.windows_lint_skipped)
        self.assertEqual(sink.getvalue(), b"check_all: skipped the Windows lint; run: x\n")


@contextlib.contextmanager
def captured_stdout(path: Path):
    """The output of this process and of the children it starts, which a Python redirect misses."""
    sys.stdout.flush()
    saved, handle = os.dup(1), os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC)
    os.dup2(handle, 1)
    try:
        yield
    finally:
        sys.stdout.flush()
        os.dup2(saved, 1)
        os.close(handle)
        os.close(saved)


# A stand-in with the three kinds of step the load rules tell apart: `unit` (no latency limit), `tm`
# (area `timing`) and the Rust editor step (area `engine`); with a host, `tm` and the editor step run
# there and `unit` here. A step named by FAKE_STEP fails while the file FAKE_NOISE exists (a loaded


if __name__ == "__main__":
    unittest.main()
