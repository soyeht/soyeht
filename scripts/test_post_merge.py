import fcntl
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import post_merge

# A stand-in for check_all.sh. The step named by FAKE_STEP (default `unit`) fails once the
# file `broken` exists; `tcc` also fails without its prerequisite `snap` in `--only`.
# FAKE_FLAKY=1 makes every `--only` run pass; FAKE_INFRA=1 makes the gate unable to start.
FAKE_GATE = """#!/bin/bash
# supports --only
only=""; base=""
while (($#)); do case "$1" in --only) only="$2"; shift 2;; --base) base="$2"; shift 2;; *) shift;; esac; done
timings="${SOYEHT_GATE_TIMINGS:?}"
step="${FAKE_STEP:-unit}"
[[ -z "${FAKE_INFRA:-}" ]] || exit 71
status=0
if [[ -n "$only" ]]; then
    [[ ",$only," == *",$step,"* ]] || exit 78
    [[ -z "${FAKE_FLAKY:-}" ]] || exit 0
    [[ "$step" != tcc || ",$only," == *",snap,"* ]] || exit 1
fi
[[ -e broken ]] && status=1
if [[ -z "$only" ]]; then
    printf '{"latest":{"steps":[{"id":"%s","area":"always","seconds":1.0,"status":%s}]}}' "$step" "$status" > "$timings"
    ((status)) && echo "check_all: FAILED step $step (area always, exit 1)" >&2
fi
exit "$status"
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


class SyntheticRepositoryTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp()) / "repo"
        self.root.mkdir()
        git(self.root, "init", "-q", "-b", "main")
        self.health = self.root.parent / "health"
        self.gate = "bash scripts/check_all.sh".split()
        self.shas = [commit(self.root, "start", {"scripts/check_all.sh": FAKE_GATE, "a": "1"})]
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


if __name__ == "__main__":
    unittest.main()
