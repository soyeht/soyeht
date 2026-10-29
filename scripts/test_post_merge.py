import fcntl
import json
import os
import unittest
from pathlib import Path
from unittest import mock

import gate_worktree
import post_merge
import post_merge_culprit
import post_merge_gate
from test_post_merge_fixtures import FAKE_GATE, SyntheticRepositoryCase, git


class CulpritSearchTests(unittest.TestCase):
    def test_finds_the_first_bad_commit(self):
        commits = [f"c{i}" for i in range(9)]
        for first_bad in range(9):
            probed = []

            def probe(commit):
                probed.append(commit)
                return post_merge_culprit.BAD if int(commit[1:]) >= first_bad else post_merge_culprit.GOOD

            result = post_merge_culprit.find_culprit(commits, probe)
            self.assertEqual(result.culprit, f"c{first_bad}")
            self.assertLessEqual(len(probed), 4)

    def test_a_single_commit_needs_no_probe(self):
        result = post_merge_culprit.find_culprit(["c0"], lambda c: self.fail("probed"))
        self.assertEqual(result.culprit, "c0")

    def test_skipped_commits_leave_candidates(self):
        commits = ["c0", "c1", "c2", "c3"]
        result = post_merge_culprit.find_culprit(commits, lambda c: post_merge_culprit.SKIP)
        self.assertIsNone(result.culprit)
        self.assertEqual(result.candidates, commits)

    def test_a_skip_next_to_the_culprit_leaves_both_as_candidates(self):
        commits = ["c0", "c1", "c2", "c3", "c4"]

        def probe(commit):
            if commit == "c2":
                return post_merge_culprit.SKIP
            return post_merge_culprit.BAD if int(commit[1:]) >= 3 else post_merge_culprit.GOOD

        result = post_merge_culprit.find_culprit(commits, probe)
        self.assertIsNone(result.culprit)
        self.assertEqual(result.candidates, ["c2", "c3"])


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
                mock.patch.object(post_merge_gate, "PREREQUISITES", {"tcc": "snap"}):
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
        stale = self.scratch / f"{gate_worktree.WORKTREE_PREFIX}stale-test"
        (stale / "tree").mkdir(parents=True)
        elsewhere = self.root.parent / f"{gate_worktree.WORKTREE_PREFIX}another-run"
        elsewhere.mkdir()
        gate_worktree.remove_stale_worktrees(self.root)
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
        (self.warm / gate_worktree.EDITOR_REPORT).write_text("{}")
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
        self.assertTrue((self.warm / gate_worktree.EDITOR_REPORT).exists())
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
        with mock.patch.object(gate_worktree, "own_checkout", return_value=self.warm.resolve() / "inner"):
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
        self.assertTrue((self.gitdir() / gate_worktree.OWNED_MARKER).is_file())

    def test_the_stale_worktree_cleanup_never_removes_the_persistent_one(self):
        warm = self.scratch / f"{gate_worktree.WORKTREE_PREFIX}warm"
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


if __name__ == "__main__":
    unittest.main()
