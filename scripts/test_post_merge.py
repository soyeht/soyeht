import contextlib
import fcntl
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import check_editor_performance
import post_merge

REAL_RUST_EDITOR_FAILURES = post_merge.rust_editor_failures

# A stand-in for check_all.sh. The step named by FAKE_STEP (default `unit`) fails once the
# file `broken` exists; `tcc` also fails without its prerequisite `snap` in `--only`.
# FAKE_FLAKY=1 makes every `--only` run pass; FAKE_INFRA=1 makes the gate unable to start;
# FAKE_TRACE=FILE records each run's directory and arguments. `--list` names a step per area:
# `eng` (engine) runs on the remote host, `unit`, `sw` and `tcc` run here.
FAKE_GATE = """#!/bin/bash
# supports --only
only=""; base=""; list=0
while (($#)); do case "$1" in --only) only="$2"; shift 2;; --base) base="$2"; shift 2;; --list) list=1; shift;; *) shift;; esac; done
if ((list)); then printf 'always both unit\nengine both eng\nswift both sw\nswift full tcc\ne2e full snap\n'; exit 0; fi
timings="${SOYEHT_GATE_TIMINGS:?}"
step="${FAKE_STEP:-unit}"
[[ -z "${FAKE_TRACE:-}" ]] || echo "$PWD only=$only base=$base" >> "$FAKE_TRACE"
[[ -z "${FAKE_INFRA:-}" ]] || exit 71
status=0
if [[ -n "$only" ]]; then
    known=0; for id in unit eng sw tcc snap; do [[ ",$only," == *",$id,"* ]] && known=1; done
    ((known)) || exit 78
    [[ ",$only," == *",$step,"* ]] || exit 0
    [[ -z "${FAKE_FLAKY:-}" ]] || exit 0
    [[ "$step" != tcc || ",$only," == *",snap,"* ]] || exit 1
fi
[[ -e broken ]] && status=1
printf '{"latest":{"steps":[{"id":"%s","area":"always","seconds":1.0,"status":%s}]}}' "$step" "$status" > "$timings"
((status)) && echo "check_all: FAILED step $step (area always, exit 1)" >&2
exit "$status"
"""


# A stand-in with an editor measurement step, run in order after `unit` and before `ts`. The step
# reads `.cache/editor-performance-vm.json` like the real check: no file fails as a missing
# report, `stale` in it (or a version other than the commit's `editor_version`) as editor files that changed, `slow` as a latency over the limit.
# `broken` makes the step named by FAKE_BREAK (default `unit`) fail.
FAKE_EDITOR_GATE = """#!/bin/bash
# supports --only
only=""; list=0
while (($#)); do case "$1" in --only) only="$2"; shift 2;; --list) list=1; shift;; *) shift;; esac; done
if ((list)); then printf 'always both unit\neditor_perf full editor-performance:check\ntypescript both ts\n'; exit 0; fi
[[ -z "${FAKE_TRACE:-}" ]] || echo "$PWD only=$only" >> "$FAKE_TRACE"
steps=""
editor() {
    local report=.cache/editor-performance-vm.json
    [[ -f "$report" ]] || { echo "editor-performance: missing $PWD/$report; run Release UI measurement on VM1" >&2; return 1; }
    # An `editor_version` file in the commit stands for its editor files: the report must name it.
    if [[ -f editor_version ]] && ! grep -q "$(cat editor_version)" "$report"; then
        echo "editor-performance: editor paths changed; measure this editor version again on VM1" >&2; return 1
    fi
    if grep -q stale "$report"; then
        echo "editor-performance: editor paths changed; measure this editor version again on VM1" >&2; return 1
    fi
    if grep -q slow "$report"; then
        echo "editor-performance: key_work_p95_ms: median 9.000 ms >= 8.0 ms; profile synchronous editor work" >&2
        return 1
    fi
}
for id in unit editor-performance:check ts; do
    [[ -z "$only" || ",$only," == *",$id,"* ]] || continue
    status=0
    if [[ "$id" == editor-performance:check ]]; then editor || status=$?
    elif [[ -e broken && "$id" == "${FAKE_BREAK:-unit}" ]]; then status=1; fi
    steps+="${steps:+,}{\\"id\\":\\"$id\\",\\"area\\":\\"always\\",\\"seconds\\":1.0,\\"status\\":$status}"
    printf '{"latest":{"steps":[%s]}}' "$steps" > "${SOYEHT_GATE_TIMINGS:?}"
    if ((status)); then echo "check_all: FAILED step $id (area always, exit $status)" >&2; exit "$status"; fi
done
"""


def git(root: Path, *arguments: str) -> str:
    return subprocess.run(
        ["git", "-c", "user.name=t", "-c", "user.email=t@example.com", *arguments],
        cwd=root, check=True, capture_output=True, text=True,
    ).stdout.strip()


def commit(root: Path, name: str, files: dict[str, str | None]) -> str:
    for path, content in files.items():
        target = root / path
        if content is None:
            target.unlink()
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content)
    git(root, "add", "-A")
    git(root, "commit", "-q", "-m", name)
    return git(root, "rev-parse", "HEAD")


class CulpritSearchTests(unittest.TestCase):
    def test_finds_the_first_bad_commit(self):
        commits = [f"c{i}" for i in range(9)]
        for first_bad in range(9):
            probed = []

            def probe(commit):
                probed.append(commit)
                return post_merge.BAD if int(commit[1:]) >= first_bad else post_merge.GOOD

            result = post_merge.find_culprit(commits, probe)
            self.assertEqual(result.culprit, f"c{first_bad}")
            self.assertLessEqual(len(probed), 4)

    def test_a_single_commit_needs_no_probe(self):
        result = post_merge.find_culprit(["c0"], lambda c: self.fail("probed"))
        self.assertEqual(result.culprit, "c0")

    def test_skipped_commits_leave_candidates(self):
        commits = ["c0", "c1", "c2", "c3"]
        result = post_merge.find_culprit(commits, lambda c: post_merge.SKIP)
        self.assertIsNone(result.culprit)
        self.assertEqual(result.candidates, commits)

    def test_a_skip_next_to_the_culprit_leaves_both_as_candidates(self):
        commits = ["c0", "c1", "c2", "c3", "c4"]

        def probe(commit):
            if commit == "c2":
                return post_merge.SKIP
            return post_merge.BAD if int(commit[1:]) >= 3 else post_merge.GOOD

        result = post_merge.find_culprit(commits, probe)
        self.assertIsNone(result.culprit)
        self.assertEqual(result.candidates, ["c2", "c3"])


class SyntheticRepositoryCase(unittest.TestCase):
    FILES = {"scripts/check_all.sh": FAKE_GATE, "a": "1"}

    def setUp(self):
        self.root = Path(tempfile.mkdtemp()) / "repo"
        self.root.mkdir()
        git(self.root, "init", "-q", "-b", "main")
        self.health = self.root.parent / "health"
        self.gate = "bash scripts/check_all.sh".split()
        self.shas = [commit(self.root, "start", self.FILES)]
        self.scratch = self.root.parent / "scratch"
        self.scratch.mkdir()
        environment = mock.patch.dict(
            os.environ,
            {"FORBIDDEN_TERMS": "placeholder", post_merge.SCRATCH_ROOT_ENV: str(self.scratch)},
        )
        environment.start()
        self.addCleanup(environment.stop)
        self.addCleanup(shutil.rmtree, self.root.parent, ignore_errors=True)

    def add(self, name, files):
        self.shas.append(commit(self.root, name, files))
        return self.shas[-1]

    def run_main(self, sha, *extra):
        return post_merge.main(
            ["--repo", str(self.root), "--sha", sha, "--health-dir", str(self.health), *extra]
        )

    def record(self, sha):
        return json.loads((self.health / f"{sha}.json").read_text())



class SyntheticRepositoryTests(SyntheticRepositoryCase):
    def test_green_main_records_result_timings_and_last_green(self):
        self.assertEqual(self.run_main(self.shas[0]), 0)
        record = json.loads((self.health / f"{self.shas[0]}.json").read_text())
        self.assertEqual(record["result"], "pass")
        self.assertEqual(record["steps"][0]["id"], "unit")
        self.assertEqual(json.loads((self.health / "latest.json").read_text())["sha"], self.shas[0])
        self.assertEqual(json.loads((self.health / "last_green.json").read_text())["sha"], self.shas[0])
        self.assertEqual(self.run_main(self.shas[0]), 0)  # already recorded: no second run

    def test_red_main_names_the_culprit_and_the_step(self):
        self.assertEqual(self.run_main(self.shas[0]), 0)
        self.add("two", {"b": "2"})
        self.add("three", {"c": "3"})
        culprit = self.add("breaks the unit step", {"broken": "x"})
        self.add("five", {"d": "4"})
        head = self.add("six", {"e": "5"})
        self.assertEqual(self.run_main(head), 1)
        record = json.loads((self.health / f"{head}.json").read_text())
        self.assertEqual(record["result"], "fail")
        self.assertEqual(record["failed_step"], "unit")
        self.assertEqual(record["culprit"]["sha"], culprit)
        self.assertEqual(record["culprit"]["subject"], "breaks the unit step")
        self.assertIn("FAILED step unit", Path(record["log"]).read_text())
        self.assertEqual(git(self.root, "rev-parse", "main"), head)  # main is untouched
        self.assertEqual(git(self.root, "worktree", "list").count("\n"), 0)  # worktree removed
        self.assertEqual(
            json.loads((self.health / "last_green.json").read_text())["sha"], self.shas[0]
        )

    def test_a_step_that_passes_when_run_again_is_flaky_and_blames_nobody(self):
        self.assertEqual(self.run_main(self.shas[0]), 0)
        self.add("two", {"b": "2"})
        head = self.add("breaks", {"broken": "x"})
        with mock.patch.dict(os.environ, {"FAKE_FLAKY": "1"}):
            self.assertEqual(self.run_main(head), 1)
        record = json.loads((self.health / f"{head}.json").read_text())
        self.assertEqual(record["result"], "flaky")
        self.assertNotIn("culprit", record)
        self.assertEqual(json.loads((self.health / "last_green.json").read_text())["sha"], self.shas[0])

    def test_a_step_is_probed_with_its_prerequisite(self):
        self.assertEqual(self.run_main(self.shas[0]), 0)
        self.add("two", {"b": "2"})
        culprit = self.add("breaks", {"broken": "x"})
        head = self.add("four", {"c": "3"})
        with mock.patch.dict(os.environ, {"FAKE_STEP": "tcc"}), \
                mock.patch.object(post_merge, "PREREQUISITES", {"tcc": "snap"}):
            self.assertEqual(self.run_main(head), 1)
        record = json.loads((self.health / f"{head}.json").read_text())
        self.assertEqual(record["failed_step"], "tcc")
        self.assertEqual(record["culprit"]["sha"], culprit)

    def test_a_gate_that_cannot_run_records_nothing(self):
        with mock.patch.dict(os.environ, {"FAKE_INFRA": "1"}):
            self.assertEqual(self.run_main(self.shas[0]), 2)
        self.assertFalse(list(self.health.glob("*.json")))

    def test_missing_vocabulary_records_nothing(self):
        with mock.patch.dict(os.environ, {"FORBIDDEN_TERMS": ""}):
            self.assertEqual(self.run_main(self.shas[0]), 2)
        self.assertFalse(self.health.exists())

    def test_only_one_run_at_a_time(self):
        self.health.mkdir(parents=True)
        with (self.health / "post_merge.lock").open("w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            self.assertEqual(self.run_main(self.shas[0]), post_merge.BUSY_EXIT)
        self.assertEqual(self.run_main(self.shas[0]), 0)

    def test_stale_worktrees_of_a_killed_run_are_removed(self):
        stale = self.scratch / f"{post_merge.WORKTREE_PREFIX}stale-test"
        (stale / "tree").mkdir(parents=True)
        elsewhere = self.root.parent / f"{post_merge.WORKTREE_PREFIX}another-run"
        elsewhere.mkdir()
        post_merge.remove_stale_worktrees(self.root)
        self.assertFalse(stale.exists())
        self.assertTrue(elsewhere.exists(), "a run's leftovers are looked for in its own root only")

    def test_no_known_green_main_reports_instead_of_searching(self):
        head = self.add("breaks", {"broken": "x"})
        self.assertEqual(self.run_main(head), 1)
        record = json.loads((self.health / f"{head}.json").read_text())
        self.assertNotIn("culprit", record)
        self.assertIn("no known green main", record["note"])

    def test_since_names_the_green_commit(self):
        self.add("two", {"b": "2"})
        culprit = self.add("breaks", {"broken": "x"})
        head = self.add("four", {"c": "3"})
        self.assertEqual(self.run_main(head, "--since", self.shas[0]), 1)
        record = json.loads((self.health / f"{head}.json").read_text())
        self.assertEqual(record["culprit"]["sha"], culprit)

    def test_commits_that_cannot_run_the_step_alone_are_reported_as_candidates(self):
        self.assertEqual(self.run_main(self.shas[0]), 0)
        # Older gates have no `--only`: the first commit after the green one is rewritten.
        self.add("old gate", {"scripts/check_all.sh": FAKE_GATE.replace("--only", "--o_n_l_y")})
        self.add("breaks", {"broken": "x"})
        head = self.add("three", {"c": "3"})
        self.assertEqual(self.run_main(head), 1)
        record = json.loads((self.health / f"{head}.json").read_text())
        self.assertNotIn("culprit", record)
        self.assertEqual([c["subject"] for c in record["candidates"]], ["old gate", "breaks", "three"])


class EditorMeasurementTests(SyntheticRepositoryCase):
    """The VM1 report is copied into the worktree; a missing or stale one is not a red main."""

    FILES = {"scripts/check_all.sh": FAKE_EDITOR_GATE, "a": "1"}

    def setUp(self):
        super().setUp()
        self.trace = self.root.parent / "trace.txt"
        self.meters = self.root.parent / "meters"
        self.meters.mkdir()
        self.editor_hash = check_editor_performance.editor_tree_hash(self.root)
        environment = mock.patch.dict(os.environ, {"FAKE_TRACE": str(self.trace)})
        environment.start()
        self.addCleanup(environment.stop)
        # The synthetic repository has no engine to benchmark; one test runs the real function.
        rust = mock.patch.object(post_merge, "rust_editor_failures", return_value=[])
        rust.start()
        self.addCleanup(rust.stop)

    def meter(self, content="ok", name="report.json", **fields):
        path = self.meters / name
        path.write_text(json.dumps({"editor_tree_hash": self.editor_hash, "state": content, **fields}))
        return path

    def runs(self):
        return self.trace.read_text().splitlines() if self.trace.exists() else []

    def green_start(self):
        self.assertEqual(self.run_main(self.shas[0], "--editor-meter", str(self.meter())), 0)
        self.trace.unlink()

    def test_the_report_is_copied_into_the_runs_worktree(self):
        self.assertEqual(self.run_main(self.shas[0], "--editor-meter", str(self.meter()), "--keep"), 0)
        self.assertEqual(self.record(self.shas[0])["editor_meter"], "copied")
        kept = next(self.scratch.glob(f"{post_merge.WORKTREE_PREFIX}*/tree"))
        self.assertEqual(json.loads((kept / ".cache/editor-performance-vm.json").read_text())["state"], "ok")

    def test_the_environment_names_the_report_too(self):
        with mock.patch.dict(os.environ, {post_merge.EDITOR_METER_ENV: str(self.meter())}):
            self.assertEqual(self.run_main(self.shas[0]), 0)
        self.assertEqual(self.record(self.shas[0])["editor_meter"], "copied")

    def test_a_missing_measurement_is_recorded_and_blames_nobody(self):
        self.green_start()
        self.add("two", {"b": "2"})
        head = self.add("three", {"c": "3"})
        self.assertEqual(self.run_main(head), post_merge.NEEDS_MEASUREMENT_EXIT)
        record = self.record(head)
        self.assertEqual(record["result"], "needs-measurement")
        self.assertEqual(record["failed_step"], "editor-performance:check")
        self.assertEqual(record["editor_tree_hash"], self.editor_hash)
        self.assertEqual(record["editor_meter"], "not-given")
        self.assertIn(self.editor_hash, record["note"])
        self.assertNotIn("culprit", record)
        self.assertNotIn("candidates", record)
        self.assertEqual([step["id"] for step in record["steps"]], ["unit", "editor-performance:check", "ts"])
        self.assertEqual(len(self.runs()), 2, "the gate, then the steps the editor check hid; no search")
        self.assertEqual(json.loads((self.health / "last_green.json").read_text())["sha"], self.shas[0])
        self.assertEqual(json.loads((self.health / "latest.json").read_text())["sha"], head)

    def test_a_report_that_is_not_there_is_the_same_as_none(self):
        head = self.add("two", {"b": "2"})
        self.assertEqual(self.run_main(head, "--editor-meter", str(self.meters / "gone.json")),
                         post_merge.NEEDS_MEASUREMENT_EXIT)
        self.assertEqual(self.record(head)["editor_meter"], "missing")

    def test_a_report_of_other_editor_files_is_a_missing_measurement(self):
        head = self.add("two", {"b": "2"})
        self.assertEqual(self.run_main(head, "--editor-meter", str(self.meter("stale"))),
                         post_merge.NEEDS_MEASUREMENT_EXIT)
        self.assertEqual(self.record(head)["result"], "needs-measurement")

    def test_a_latency_over_the_limit_stays_red(self):
        self.green_start()
        head = self.add("slows the editor", {"b": "2"})
        self.assertEqual(self.run_main(head, "--editor-meter", str(self.meter("slow"))), 1)
        record = self.record(head)
        self.assertEqual(record["result"], "fail")
        self.assertEqual(record["failed_step"], "editor-performance:check")
        self.assertEqual(record["culprit"]["sha"], head)
        self.assertNotIn("editor_tree_hash", record)

    def test_a_step_after_the_editor_check_still_decides_the_run(self):
        self.green_start()
        head = self.add("breaks the last step", {"broken": "x"})
        with mock.patch.dict(os.environ, {"FAKE_BREAK": "ts"}):
            self.assertEqual(self.run_main(head), 1)
        record = self.record(head)
        self.assertEqual((record["result"], record["failed_step"]), ("fail", "ts"))
        self.assertEqual(record["culprit"]["sha"], head)

    def test_a_recorded_run_waits_for_a_report_of_its_hash(self):
        head = self.add("two", {"b": "2"})
        self.assertEqual(self.run_main(head), post_merge.NEEDS_MEASUREMENT_EXIT)
        runs = len(self.runs())
        self.assertEqual(self.run_main(head), post_merge.NEEDS_MEASUREMENT_EXIT)  # nothing new to say
        other = self.meter(name="other.json", editor_tree_hash="0" * 64)
        self.assertEqual(self.run_main(head, "--editor-meter", str(other)), post_merge.NEEDS_MEASUREMENT_EXIT)
        self.assertEqual(len(self.runs()), runs, "no second run until the report measures this hash")
        self.assertEqual(self.run_main(head, "--editor-meter", str(self.meter())), 0)
        self.assertEqual(self.record(head)["result"], "pass")
        self.assertGreater(len(self.runs()), runs)

    def test_the_rust_half_of_the_editor_check_still_runs_and_its_failure_is_red(self):
        head = self.add("two", {"b": "2"})
        for failures, expected in (([], post_merge.NEEDS_MEASUREMENT_EXIT), (["open_100k_ms=200 ms >= 150 ms"], 1)):
            with mock.patch.object(post_merge, "rust_editor_failures", return_value=failures) as rust:
                self.assertEqual(self.run_main(head, "--force"), expected)
            rust.assert_called_once()
        record = self.record(head)
        self.assertEqual((record["result"], record["failed_step"]), ("fail", post_merge.EDITOR_RUST_STEP))
        self.assertIn("open_100k_ms=200", record["note"])
        self.assertNotIn("culprit", record)
        self.assertNotIn("candidates", record)  # no search: the step cannot be probed alone

    def test_the_rust_benchmark_is_run_with_the_limits_of_the_editor_check(self):
        bin_dir = self.root.parent / "bin"
        bin_dir.mkdir()
        cargo = bin_dir / "cargo"
        cargo.write_text(
            '#!/bin/bash\nreport="${@: -1}"\n[[ "${FAKE_CARGO_EXIT:-0}" == 0 ]] || exit "$FAKE_CARGO_EXIT"\n'
            'echo "$PWD $*" >> "$FAKE_CARGO_LOG"\n'
            'printf \'{"open_100k_ms": %s, "search_1gb_ms": 10}\' "${FAKE_OPEN_MS:-1}" > "$report"\n')
        cargo.chmod(0o755)
        tree = self.root.parent / "tree"
        (tree / "engine").mkdir(parents=True)
        log = self.root.parent / "rust.log"
        environment = {"PATH": f"{bin_dir}:{os.environ['PATH']}", "FAKE_CARGO_LOG": str(self.root.parent / "cargo.txt")}
        with mock.patch.dict(os.environ, environment):
            self.assertEqual(REAL_RUST_EDITOR_FAILURES(tree, log), [])
            with mock.patch.dict(os.environ, {"FAKE_OPEN_MS": "150.0"}):
                self.assertEqual(len(REAL_RUST_EDITOR_FAILURES(tree, log)), 1)
            with mock.patch.dict(os.environ, {"FAKE_CARGO_EXIT": "3"}):
                self.assertIn("exited with 3", REAL_RUST_EDITOR_FAILURES(tree, log)[0])
        call = (self.root.parent / "cargo.txt").read_text().splitlines()[0]
        self.assertTrue(call.startswith(str((tree / "engine").resolve())))
        self.assertIn("-p soyeht-editor --example editor_performance -- --report", call)

    def test_a_stale_report_on_an_older_commit_does_not_blame_it(self):
        """The probes run older commits with the newest report: stale says nothing about latency."""
        self.add("editor v1", {"editor_version": "v1"})
        self.assertEqual(self.run_main(self.shas[-1], "--editor-meter", str(self.meter("ok v1"))), 0)
        self.add("unrelated", {"b": "2"})
        self.add("editor v2", {"editor_version": "v2"})
        self.add("unrelated too", {"c": "3"})
        head = self.add("unrelated again", {"d": "4"})
        self.assertEqual(self.run_main(head, "--editor-meter", str(self.meter("slow v2"))), 1)
        record = self.record(head)
        self.assertEqual((record["result"], record["failed_step"]), ("fail", "editor-performance:check"))
        self.assertNotIn("culprit", record, "a commit that fails as stale is skipped, not blamed")
        self.assertEqual([c["subject"] for c in record["candidates"]], ["unrelated", "editor v2"])


class EditorMessageContractTests(unittest.TestCase):
    """The log lines that make a run `needs-measurement` are the ones check_editor_performance prints."""

    def printed(self, vm_report: Path) -> str:
        output = io.StringIO()
        with mock.patch.object(check_editor_performance, "VM_REPORT", vm_report), \
                mock.patch.object(sys, "argv", ["check_editor_performance.py"]), \
                contextlib.redirect_stderr(output):
            self.assertEqual(check_editor_performance.main(), 1)
        return output.getvalue()

    def log_of(self, text: str) -> Path:
        log = Path(tempfile.mkdtemp()) / "gate.log"
        self.addCleanup(shutil.rmtree, log.parent, ignore_errors=True)
        log.write_text(text)
        return log

    def test_a_missing_report_is_recognised(self):
        message = self.printed(Path(tempfile.gettempdir()) / "no-such-report" / "editor.json")
        self.assertTrue(message.startswith(post_merge.MISSING_REPORT), message)
        self.assertTrue(post_merge.lacks_measurement(self.log_of(message)))

    def test_a_report_of_other_editor_files_is_recognised(self):
        directory = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, directory, ignore_errors=True)
        report = directory / "editor.json"
        report.write_text(json.dumps({"configuration": "Release", "editor_tree_hash": "of another version"}))
        message = self.printed(report)
        self.assertIn(post_merge.STALE_REPORT, message)
        self.assertTrue(post_merge.lacks_measurement(self.log_of(message)))
        self.assertIn(post_merge.STALE_REPORT, check_editor_performance.report_failures({}, "hash"))

    def test_a_latency_over_the_limit_is_not_a_missing_measurement(self):
        run = {"key_event_samples": 240, "validated_key_samples": 240, "key_work_samples": 240,
               "visual_validation": "real_window_screenshot_and_gutter", "file_line_count": 100_000,
               "key_work_p95_ms": 9.0}
        report = {"configuration": "Release", "editor_tree_hash": "h", "unmeasured": check_editor_performance.UNMEASURED,
                  "runs": [run] * check_editor_performance.RUN_COUNT}
        failures = check_editor_performance.report_failures(report, "h")
        self.assertEqual(len(failures), 1)
        self.assertFalse(post_merge.lacks_measurement(self.log_of("editor-performance: " + "; ".join(failures))))


class PersistentWorktreeTests(SyntheticRepositoryCase):
    """--worktree: one worktree between runs, with its build directories."""

    def setUp(self):
        super().setUp()
        self.warm = self.root.parent / "warm"
        self.trace = self.root.parent / "trace.txt"
        environment = mock.patch.dict(os.environ, {"FAKE_TRACE": str(self.trace)})
        environment.start()
        self.addCleanup(environment.stop)

    def directories(self):
        return {Path(line.split(" ", 1)[0]).resolve() for line in self.trace.read_text().splitlines()}

    def test_the_worktree_is_reused_and_keeps_only_what_is_safe_to_reuse(self):
        self.assertEqual(self.run_main(self.shas[0], "--worktree", str(self.warm)), 0)
        kept = ("engine/target", "apps/desktop/target", ".cache/apple-packages")
        stale = ("apps/shared/.build", "apps/dictation/.build", ".cache/mac-derived", ".cache/ios-derived")
        for directory in (*kept, *stale):  # SwiftPM and Xcode builds are not safe across checkouts
            (self.warm / directory).mkdir(parents=True)
            (self.warm / directory / "artifact").write_text("built")
        (self.warm / post_merge.EDITOR_REPORT).write_text("{}")
        (self.warm / ".cache/e2e.json").write_text("{}")
        (self.warm / "stray").write_text("x")
        (self.warm / "ignored" / "deep").mkdir(parents=True)
        head = self.add("two", {"b": "2"})
        self.assertEqual(self.run_main(head, "--worktree", str(self.warm)), 0)
        self.assertEqual(git(self.warm, "rev-parse", "HEAD"), head)
        for gone in ("stray", "ignored", ".cache/e2e.json", *stale):
            self.assertFalse((self.warm / gone).exists(), gone)
        for directory in kept:
            self.assertEqual((self.warm / directory / "artifact").read_text(), "built")
        self.assertTrue((self.warm / post_merge.EDITOR_REPORT).exists())
        self.assertEqual(self.directories(), {self.warm.resolve()})
        self.assertIn(str(self.warm.resolve()), git(self.root, "worktree", "list"))  # not removed

    def test_the_environment_names_the_worktree_too(self):
        with mock.patch.dict(os.environ, {post_merge.WORKTREE_ENV: str(self.warm)}):
            self.assertEqual(self.run_main(self.shas[0]), 0)
        self.assertEqual(self.directories(), {self.warm.resolve()})

    def test_the_culprit_search_probes_in_the_same_worktree(self):
        self.assertEqual(self.run_main(self.shas[0], "--worktree", str(self.warm)), 0)
        self.add("two", {"b": "2"})
        self.add("three", {"c": "3"})
        culprit = self.add("breaks the unit step", {"broken": "x"})
        head = self.add("five", {"d": "4"})
        self.trace.unlink()
        self.assertEqual(self.run_main(head, "--worktree", str(self.warm)), 1)
        self.assertEqual(self.record(head)["culprit"]["sha"], culprit)
        self.assertGreater(len(self.trace.read_text().splitlines()), 2, "the rerun and the probes ran")
        self.assertEqual(self.directories(), {self.warm.resolve()})

    def test_the_main_checkout_and_a_stranger_directory_are_refused(self):
        (self.root / "precious").write_text("work in progress")
        self.assertEqual(self.run_main(self.shas[0], "--worktree", str(self.root)), 2)
        self.assertEqual((self.root / "precious").read_text(), "work in progress")
        inside = self.root / "inside"
        self.assertEqual(self.run_main(self.shas[0], "--worktree", str(inside)), 2)
        self.assertFalse(inside.exists())
        stranger = self.root.parent / "stranger"
        stranger.mkdir()
        (stranger / "file").write_text("x")
        self.assertEqual(self.run_main(self.shas[0], "--worktree", str(stranger)), 2)
        self.assertTrue((stranger / "file").exists())
        elsewhere = self.root.parent / "other-repo"
        elsewhere.mkdir()
        git(elsewhere, "init", "-q")
        self.assertEqual(self.run_main(self.shas[0], "--worktree", str(elsewhere)), 2)
        self.assertFalse(list(self.health.glob("*.json")))

    def test_a_worktree_of_the_repository_that_this_script_did_not_create_is_never_touched(self):
        other = self.root.parent / "agent-worktree"
        git(self.root, "worktree", "add", "-q", "--detach", str(other), self.shas[0])
        (other / "unsaved-work").write_text("hours of work")
        (other / "a").write_text("edited, not committed")
        head = self.add("two", {"b": "2"})
        self.assertEqual(self.run_main(head, "--worktree", str(other)), 2)
        self.assertEqual((other / "unsaved-work").read_text(), "hours of work")
        self.assertEqual((other / "a").read_text(), "edited, not committed")
        self.assertEqual(git(other, "rev-parse", "HEAD"), self.shas[0])
        self.assertFalse(list(self.health.glob("*.json")))

    def test_the_worktree_this_script_runs_from_is_refused(self):
        self.assertEqual(self.run_main(self.shas[0], "--worktree", str(self.warm)), 0)
        with mock.patch.object(post_merge, "own_checkout", return_value=self.warm.resolve() / "inner"):
            self.assertEqual(self.run_main(self.shas[0], "--worktree", str(self.warm), "--force"), 2)

    def test_a_relative_path_names_the_same_directory_for_every_check(self):
        elsewhere = self.root.parent / "cwd"
        elsewhere.mkdir()
        previous = os.getcwd()
        os.chdir(elsewhere)
        self.addCleanup(os.chdir, previous)
        self.assertEqual(self.run_main(self.shas[0], "--worktree", "relwt"), 0)
        self.assertTrue((elsewhere / "relwt/.git").exists())
        self.assertFalse((self.root / "relwt").exists())
        head = self.add("two", {"b": "2"})
        self.assertEqual(self.run_main(head, "--worktree", "relwt"), 0)
        self.assertEqual(git(elsewhere / "relwt", "rev-parse", "HEAD"), head)

    def test_a_path_that_cannot_be_created_is_a_host_problem_not_a_crash(self):
        blocker = self.root.parent / "a-file"
        blocker.write_text("x")
        self.assertEqual(self.run_main(self.shas[0], "--worktree", str(blocker / "child")), 2)
        self.assertFalse(list(self.health.glob("*.json")))

    def gitdir(self):
        pointer = (self.warm / ".git").read_text().split(":", 1)[1].strip()
        return Path(pointer)

    def test_a_killed_git_leaves_a_lock_that_the_next_run_removes(self):
        self.assertEqual(self.run_main(self.shas[0], "--worktree", str(self.warm)), 0)
        (self.gitdir() / "index.lock").write_text("")
        head = self.add("two", {"b": "2"})
        self.assertEqual(self.run_main(head, "--worktree", str(self.warm)), 0)
        self.assertFalse((self.gitdir() / "index.lock").exists())
        self.assertEqual(git(self.warm, "rev-parse", "HEAD"), head)

    def test_a_worktree_that_cannot_be_prepared_is_recreated(self):
        self.assertEqual(self.run_main(self.shas[0], "--worktree", str(self.warm)), 0)
        (self.gitdir() / "HEAD").write_text("garbage\n")  # git fails in the worktree from now on
        head = self.add("two", {"b": "2"})
        self.assertEqual(self.run_main(head, "--worktree", str(self.warm)), 0)
        self.assertEqual(git(self.warm, "rev-parse", "HEAD"), head)
        self.assertTrue((self.gitdir() / post_merge.OWNED_MARKER).is_file())

    def test_the_stale_worktree_cleanup_never_removes_the_persistent_one(self):
        warm = self.scratch / f"{post_merge.WORKTREE_PREFIX}warm"
        self.assertEqual(self.run_main(self.shas[0], "--worktree", str(warm)), 0)
        (warm / ".cache").mkdir(exist_ok=True)
        (warm / "engine/target").mkdir(parents=True)
        (warm / "engine/target/artifact").write_text("built")
        head = self.add("two", {"b": "2"})
        self.assertEqual(self.run_main(head, "--worktree", str(warm)), 0)
        self.assertEqual((warm / "engine/target/artifact").read_text(), "built")
        self.assertEqual(git(warm, "rev-parse", "HEAD"), head)

    def test_a_temporary_worktree_is_still_removed(self):
        self.assertEqual(self.run_main(self.shas[0]), 0)
        self.assertEqual(git(self.root, "worktree", "list").count("\n"), 0)
        self.assertFalse(list(self.scratch.iterdir()))


FAKE_SSH = """#!/bin/bash
# Runs the remote command here, as the host would; the first argument is the host name.
[[ -z "${FAKE_SSH_LOG:-}" ]] || echo "$1" >> "$FAKE_SSH_LOG"
shift
exec bash -c "$*"
"""


class RemoteGateTests(SyntheticRepositoryCase):
    """--remote: engine, desktop and e2e steps through scripts/remote_gate.sh, the rest here."""

    FILES = {
        **SyntheticRepositoryCase.FILES,
        "scripts/remote_gate.sh": (Path(__file__).with_name("remote_gate.sh")).read_text(),
    }

    def setUp(self):
        super().setUp()
        self.host_checkout = self.root.parent / "host"
        subprocess.run(["git", "clone", "-q", str(self.root), str(self.host_checkout)], check=True)
        self.fake_ssh = self.root.parent / "fake_ssh"
        self.fake_ssh.write_text(FAKE_SSH)
        self.fake_ssh.chmod(0o755)
        self.trace = self.root.parent / "trace.txt"
        self.ssh_log = self.root.parent / "ssh.txt"
        environment = mock.patch.dict(os.environ, {
            post_merge.SSH_ENV: str(self.fake_ssh), "FAKE_TRACE": str(self.trace),
            "FAKE_SSH_LOG": str(self.ssh_log),
        })
        environment.start()
        self.addCleanup(environment.stop)

    def run_remote(self, sha, *extra, root=None):
        return self.run_main(sha, "--remote", "linux-host", "--remote-root",
                             str(root or self.host_checkout), *extra)

    def runs(self):
        return [line.split(" ", 1) for line in self.trace.read_text().splitlines()]

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
        with mock.patch.dict(os.environ, {post_merge.SSH_ENV: str(broken_ssh)}):
            self.assertEqual(self.run_remote(self.shas[0]), 2)
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


if __name__ == "__main__":
    unittest.main()
