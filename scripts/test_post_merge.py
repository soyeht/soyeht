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

REAL_REMOTE_LOAD = post_merge.remote_load  # LoadRuleTests replaces the module's

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


def isolate_environment():
    """Inside a `mock.patch.dict(os.environ)`: forget the variables post_merge reads, which a
    watch wrapper may export (they would send the tests to a real worktree, meter or host)."""
    for name in (post_merge.WORKTREE_ENV, post_merge.EDITOR_METER_ENV, post_merge.SSH_ENV):
        os.environ.pop(name, None)


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
        isolate_environment()
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


class ForcedRerunTests(SyntheticRepositoryCase):
    """A `--force` rerun keeps the evidence of the run it replaces."""

    def test_the_replaced_run_moves_to_the_history_and_the_new_record_names_it(self):
        head = self.add("breaks", {"broken": "x"})
        self.assertEqual(self.run_main(head), 1)
        first = self.record(head)
        first_log = Path(first["log"]).read_text()
        self.assertIn("FAILED step unit", first_log)
        (self.root / "broken").unlink()  # the same commit now behaves differently, as a flake does
        with mock.patch.dict(os.environ, {"FAKE_FLAKY": "1"}):
            self.assertEqual(self.run_main(head, "--force"), 1)
        stem = self.health / "history" / f"{head}.{first['started_at'].replace(':', '')}"
        self.assertEqual(Path(f"{stem}.log").read_text(), first_log)
        self.assertEqual(json.loads(Path(f"{stem}.json").read_text()), first)
        self.assertTrue(Path(f"{stem}.timings.json").exists())
        previous = self.record(head)["previous"]
        self.assertEqual(previous["result"], "fail")
        self.assertEqual(previous["failed_step"], "unit")
        self.assertEqual(previous["started_at"], first["started_at"])
        self.assertEqual(previous["log"], f"{stem}.log")

    def test_a_second_archive_of_the_same_record_keeps_the_first(self):
        head = self.add("breaks", {"broken": "x"})
        self.assertEqual(self.run_main(head), 1)
        first_log = Path(self.record(head)["log"]).read_text()
        # Reruns that die before they write a record: the record keeps its `started_at`.
        with mock.patch.dict(os.environ, {"FAKE_INFRA": "1"}):
            self.assertEqual(self.run_main(head, "--force"), 2)
            self.assertEqual(self.run_main(head, "--force"), 2)
        logs = sorted((self.health / "history").glob("*.log"))
        self.assertEqual(len(logs), 2)
        self.assertIn(first_log, [path.read_text() for path in logs])
        self.assertEqual(len({path.stem for path in logs}), 2)

    def test_a_first_run_has_no_previous_and_no_history(self):
        self.assertEqual(self.run_main(self.shas[0]), 0)
        self.assertNotIn("previous", self.record(self.shas[0]))
        self.assertFalse((self.health / "history").exists())

    def test_a_rerun_that_cannot_start_leaves_the_record_in_place(self):
        self.assertEqual(self.run_main(self.shas[0]), 0)
        before = self.record(self.shas[0])
        with mock.patch.dict(os.environ, {"FAKE_INFRA": "1"}):
            self.assertEqual(self.run_main(self.shas[0], "--force"), 2)
        self.assertEqual(self.record(self.shas[0]), before)
        self.assertEqual(len(list((self.health / "history").glob("*.log"))), 1)


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


class RemoteHostCase(SyntheticRepositoryCase):
    """A repository whose clone stands in for the Linux host, reached through a fake ssh."""

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
        with mock.patch.dict(os.environ, {post_merge.SSH_ENV: str(broken_ssh)}):
            self.assertEqual(self.run_remote(self.shas[0]), 2)
        self.assertFalse(list(self.health.glob("*.json")))

    def test_a_root_with_a_tilde_is_the_hosts_home(self):
        with mock.patch.dict(os.environ, {"HOME": str(self.root.parent)}):
            self.assertEqual(self.run_remote(self.shas[0], root="~/host"), 0)
        self.assertEqual(post_merge.Remote("h", "~").remote_path(), '"$HOME"\'\'')
        self.assertEqual(post_merge.Remote("h", "/a b").remote_path(), "'/a b'")

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
        with mock.patch.dict(os.environ, {post_merge.SSH_ENV: str(flaky_ssh), "FAKE_STEP": "eng"}):
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
        with mock.patch.dict(os.environ, {post_merge.SSH_ENV: str(busy_ssh)}):
            self.assertEqual(self.run_remote(self.shas[0]), post_merge.INFRASTRUCTURE_EXIT)
            self.add("two", {"b": "2"})
            head = self.add("three", {"c": "3"})
            self.assertEqual(self.run_remote(head), post_merge.INFRASTRUCTURE_EXIT)
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
        load = post_merge.remote_load(post_merge.Remote("linux-host", str(self.host_checkout)))
        self.assertEqual((load.host, load.cpus >= 1), ("linux-host", True))

    def test_a_bad_slot_name_is_refused(self):
        result = self.script_run("--sha", self.shas[0], "--steps", "unit", "--slot", "../x")
        self.assertEqual(result.returncode, 64)


# A stand-in with the three kinds of step the load rules tell apart: `unit` (no latency limit), `tm`
# (area `timing`, runs on this machine) and the Rust editor step (area `engine`, runs on the host).
# A step named by FAKE_STEP fails while the file FAKE_NOISE exists (a loaded host) or `broken` does.
# On the host (the checkout `remote-gate`) the noise is not heard: that host is quiet.
FAKE_LOAD_GATE = """#!/bin/bash
# supports --only
only=""; list=0
while (($#)); do case "$1" in --only) only="$2"; shift 2;; --list) list=1; shift;; *) shift;; esac; done
if ((list)); then printf 'always both unit\\ntiming full tm\\nengine full editor-performance:rust\\n'; exit 0; fi
step="${FAKE_STEP:-tm}"
echo "$PWD only=$only" >> "${FAKE_TRACE:?}"
[[ -z "$only" || ",$only," == *",$step,"* ]] || exit 0
status=0
[[ -e broken ]] && status=1
[[ -n "${FAKE_NOISE:-}" && -e "$FAKE_NOISE" && "$PWD" != *remote-gate* ]] && status=1
printf '{"latest":{"steps":[{"id":"%s","area":"x","seconds":1.0,"status":%s}]}}' "$step" "$status" > "${SOYEHT_GATE_TIMINGS:?}"
((status)) && echo "check_all: FAILED step $step (area x, exit 1)" >&2
exit "$status"
"""

BUSY, QUIET = post_merge.Load("local", 80.0, 8), post_merge.Load("local", 2.0, 8)


class LoadRuleTests(RemoteHostCase):
    """A timing step that fails on a busy host is rerun on a quiet one before main is called red."""

    FILES = {
        "scripts/check_all.sh": FAKE_LOAD_GATE, "a": "1",
        "scripts/remote_gate.sh": (Path(__file__).with_name("remote_gate.sh")).read_text(),
    }

    def setUp(self):
        super().setUp()
        self.noise = self.root.parent / "noise"
        environment = mock.patch.dict(os.environ, {"FAKE_NOISE": str(self.noise)})
        environment.start()
        self.addCleanup(environment.stop)
        self.pauses = []

        def pause(seconds):
            self.pauses.append(seconds)
            self.noise.unlink(missing_ok=True)  # the load drops while we wait

        for target, replacement in (
            ("pause", pause),
            ("local_load", lambda: BUSY if self.noise.exists() else QUIET),
            ("remote_load", lambda remote: post_merge.Load(remote.host, 1.0, 32)),
        ):
            patch = mock.patch.object(post_merge, target, replacement)
            patch.start()
            self.addCleanup(patch.stop)
        self.assertEqual(self.run_main(self.shas[0]), 0)
        self.trace.unlink()

    def fail_under_load(self, step, *extra):
        self.add("two", {"b": "2"})
        head = self.add("three", {"c": "3"})
        self.noise.write_text("")
        with mock.patch.dict(os.environ, {"FAKE_STEP": step}):
            return head, self.run_main(head, *extra)

    def flaky_record_under_load(self, *extra):
        """The record of a timing step that failed on a busy host and passed on the rerun."""
        head, code = self.fail_under_load("tm", *extra)
        self.assertEqual(code, 1)
        record = self.record(head)
        self.assertEqual(record["result"], "flaky")
        return record

    def probe_record(self, head, local_load):
        """The record of a run whose culprit search reads the Mac's load from `local_load`."""
        with mock.patch.object(post_merge, "local_load", local_load), \
                mock.patch.dict(os.environ, {"FAKE_STEP": "tm"}):
            self.assertEqual(self.run_main(head, "--since", self.shas[0]), 1)
        return self.record(head)

    def test_a_timing_step_that_failed_on_a_busy_host_is_rerun_after_the_load_drops(self):
        record = self.flaky_record_under_load()
        self.assertEqual(record["load"], {"host": "local", "average": 80.0, "cpus": 8})
        self.assertEqual(record["load_before_rerun"]["average"], 2.0)
        self.assertIn("busy; rerun after waiting", record["note"])
        self.assertEqual(self.pauses, [post_merge.LOAD_POLL_SECONDS])
        self.assertNotIn("culprit", record)

    def test_a_timing_step_that_failed_on_a_busy_host_is_rerun_on_the_remote_host(self):
        remote = ("--remote", "linux-host", "--remote-root", str(self.host_checkout))
        record = self.flaky_record_under_load(*remote)
        self.assertIn("rerun on linux-host", record["note"])
        self.assertEqual(self.pauses, [], "a quiet host needs no wait")
        directory, arguments = self.runs()[-1]
        self.assertIn("remote-gate/tree", directory)
        self.assertEqual(arguments, "only=tm")

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
        self.add("two", {"b": "2"})
        culprit = self.add("slows it down", {"broken": "x"})
        self.add("four", {"c": "3"})
        head = self.add("five", {"d": "4"})
        with mock.patch.dict(os.environ, {"FAKE_STEP": "tm"}), \
                mock.patch.object(post_merge, "wait_until_quiet", return_value=QUIET) as waited:
            self.assertEqual(self.run_main(head, "--since", self.shas[0]), 1)
        self.assertEqual(self.record(head)["culprit"]["sha"], culprit)
        # The gate, the rerun, then one run per probe; each probe was preceded by a wait.
        probes = len(self.trace.read_text().splitlines()) - 2
        self.assertGreaterEqual(probes, 2)
        self.assertEqual(waited.call_count, probes)

    def test_the_rust_editor_step_runs_on_the_remote_host_with_the_other_engine_steps(self):
        head = self.add("two", {"b": "2"})
        with mock.patch.dict(os.environ, {"FAKE_STEP": "editor-performance:rust"}):
            self.assertEqual(self.run_remote(head), 0)
        (remote_directory, remote_arguments), (local_directory, local_arguments) = self.runs()
        self.assertIn("remote-gate/tree", remote_directory)
        self.assertEqual(remote_arguments.split()[0], "only=editor-performance:rust")
        self.assertNotIn("remote-gate", local_directory)
        self.assertEqual(local_arguments.split()[0], "only=unit,tm")

    def test_a_remote_step_is_judged_by_the_load_of_the_host_it_ran_on(self):
        self.add("two", {"b": "2"})
        head = self.add("three", {"c": "3"})
        self.noise.write_text("")  # the Mac is busy; the host does not hear it, and its own load is quiet
        with mock.patch.dict(os.environ, {"FAKE_STEP": "editor-performance:rust", "FAKE_NOISE": ""}):
            self.assertEqual(self.run_remote(head), 0)
        self.assertEqual(self.pauses, [])
        self.assertEqual(self.record(head)["local_load"]["average"], 80.0)

    def test_load_is_busy_per_processor_and_the_wait_is_bounded(self):
        self.assertTrue(post_merge.Load("h", 16.1, 16).busy())
        self.assertFalse(post_merge.Load("h", 16.0, 16).busy())
        with mock.patch.dict(os.environ, {post_merge.LOAD_LIMIT_ENV: "4"}):
            self.assertFalse(post_merge.Load("h", 60.0, 16).busy())
        waits = []
        budget = post_merge.WaitBudget()
        with mock.patch.object(post_merge, "pause", waits.append), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(post_merge.wait_until_quiet(lambda: BUSY, budget), BUSY)
            self.assertEqual(post_merge.wait_until_quiet(lambda: BUSY, budget), BUSY, "the budget is shared")
            self.assertIsNone(post_merge.wait_until_quiet(lambda: None, budget), "a host that cannot say is not waited for")
        self.assertEqual(sum(waits), post_merge.LOAD_WAIT_BUDGET_SECONDS)

    # The cases of the review of the first version: a host that never quiets down, or cannot be asked.

    def stay_busy(self):
        """The Mac never quiets down: waiting does not clear the noise."""
        patch = mock.patch.object(post_merge, "pause", self.pauses.append)
        patch.start()
        self.addCleanup(patch.stop)

    def remote_options(self):
        return ("--remote", "linux-host", "--remote-root", str(self.host_checkout))

    def test_probes_of_a_step_rerun_on_the_host_run_on_the_host_too(self):
        self.stay_busy()
        self.add("two", {"b": "2"})
        culprit = self.add("slows it down", {"broken": "x"})
        self.add("four", {"c": "3"})
        head = self.add("five", {"d": "4"})
        self.noise.write_text("")  # the Mac is busy for good; the host is quiet
        with mock.patch.dict(os.environ, {"FAKE_STEP": "tm"}):
            self.assertEqual(self.run_main(head, "--since", self.shas[0], *self.remote_options()), 1)
        record = self.record(head)
        self.assertEqual(record["culprit"]["sha"], culprit, "a busy Mac must not fail a good commit")
        runs = self.runs()
        # The first run (the engine step on the host, the rest here) and the rerun are not probes.
        probes = runs[3:]
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
        self.assertEqual(sum(self.pauses), post_merge.LOAD_WAIT_BUDGET_SECONDS)
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
        self.assertEqual(sum(self.pauses), post_merge.LOAD_WAIT_BUDGET_SECONDS, "one budget for all the probes")

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

    def test_an_unreachable_host_is_not_waited_for_and_the_rerun_waits_here(self):
        with mock.patch.object(post_merge, "remote_load", lambda remote: None):
            record = self.flaky_record_under_load(*self.remote_options())
        self.assertIn("rerun after waiting", record["note"])
        self.assertEqual(self.pauses, [post_merge.LOAD_POLL_SECONDS], "the wait is the Mac's only")

    def test_a_busy_host_uses_the_budget_and_leaves_a_busy_mac_inconclusive(self):
        self.stay_busy()
        with mock.patch.object(post_merge, "remote_load", lambda remote: post_merge.Load(remote.host, 90.0, 8)):
            record = self.flaky_record_under_load(*self.remote_options())
        self.assertIn("inconclusive", record["note"])
        self.assertEqual(sum(self.pauses), post_merge.LOAD_WAIT_BUDGET_SECONDS)

    def test_a_host_rerun_that_cannot_run_falls_back_to_waiting_on_the_mac(self):
        self.add("two", {"b": "2"})
        head = self.add("three", {"c": "3"})
        seen = self.root.parent / "gate-calls"
        busy_ssh = self.root.parent / "busy_ssh"
        busy_ssh.write_text(f"""#!/bin/bash
# The first gate call goes through; every later one finds the host's worktree busy (exit 75).
if [[ "$*" == *"--sha"* ]]; then
    [[ -e {seen} ]] && exit 75
    touch {seen}
fi
exec {self.fake_ssh} "$@"
""")
        busy_ssh.chmod(0o755)
        self.noise.write_text("")
        with mock.patch.dict(os.environ, {post_merge.SSH_ENV: str(busy_ssh), "FAKE_STEP": "tm"}):
            self.assertEqual(self.run_main(head, *self.remote_options()), 1)
        record = self.record(head)
        self.assertEqual(record["result"], "flaky")
        self.assertIn("rerun after waiting", record["note"])
        self.assertEqual(self.pauses, [post_merge.LOAD_POLL_SECONDS])

    def test_asking_a_silent_host_for_its_load_times_out(self):
        remote = post_merge.Remote("linux-host", str(self.host_checkout))
        with mock.patch.object(post_merge.subprocess, "run",
                               side_effect=subprocess.TimeoutExpired("ssh", 1)) as run:
            self.assertIsNone(REAL_REMOTE_LOAD(remote))
        self.assertEqual(run.call_args.kwargs["timeout"], post_merge.LOAD_PROBE_TIMEOUT_SECONDS)


if __name__ == "__main__":
    unittest.main()
