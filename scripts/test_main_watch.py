import fcntl
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import gate_flakes
import gate_worktree
import main_watch
import post_merge
import post_merge_editor
import post_merge_gate
import post_merge_records
from test_post_merge_fixtures import FAKE_EDITOR_GATE, FAKE_GATE, commit, git, isolate_environment

# A stand-in for the GitHub CLI. `pr create` records what the pushed branch changes relative to
# main and what it says; `pr merge` squashes that branch into main of the bare origin, like the
# hosted merge would. FAKE_GH_FAIL_MERGE=1 makes the merge fail.
FAKE_GH = """#!/bin/bash
echo "$*" | tr '\n' ' ' >> "$FAKE_GH_DIR/calls.txt"; echo >> "$FAKE_GH_DIR/calls.txt"
identity=(-c user.name=t -c user.email=t@example.com)
case "$1 $2" in
    "pr create")
        while (($#)); do
            case "$1" in --head) head="$2"; shift 2;; --title) title="$2"; shift 2;; --body) body="$2"; shift 2;; *) shift;; esac
        done
        printf '%s\\n' "$title" > "$FAKE_GH_DIR/title.txt"
        printf '%s\\n' "$body" > "$FAKE_GH_DIR/body.txt"
        git --git-dir "$FAKE_ORIGIN" diff --name-status refs/heads/main "refs/heads/$head" > "$FAKE_GH_DIR/changes.txt"
        git --git-dir "$FAKE_ORIGIN" log -1 --format=%B "refs/heads/$head" > "$FAKE_GH_DIR/commit.txt"
        echo "$head" > "$FAKE_GH_DIR/head.txt"
        echo "https://example.test/pull/7"
        ;;
    "pr merge")
        [[ -z "${FAKE_GH_FAIL_MERGE:-}" ]] || { echo "merge blocked" >&2; exit 1; }
        head="$(cat "$FAKE_GH_DIR/head.txt")"
        work="$(mktemp -d)"
        git clone -q "$FAKE_ORIGIN" "$work"
        git -C "$work" merge -q --squash "origin/$head"
        git -C "$work" "${identity[@]}" commit -q -m "$(cat "$FAKE_GH_DIR/title.txt") (#7)"
        git -C "$work" push -q origin HEAD:main
        ;;
esac
"""


class WatchCase(unittest.TestCase):
    def setUp(self):
        self.base = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.base, ignore_errors=True)
        self.origin = self.base / "origin.git"
        subprocess.run(["git", "init", "-q", "--bare", "-b", "main", str(self.origin)], check=True)
        self.root = self.base / "repo"
        subprocess.run(["git", "clone", "-q", str(self.origin), str(self.root)], check=True,
                       capture_output=True)
        git(self.root, "checkout", "-q", "-b", "main")
        self.health = self.base / "health"
        self.scratch = self.base / "scratch"
        self.scratch.mkdir()
        self.gh_dir = self.base / "gh"
        self.gh_dir.mkdir()
        self.gh = self.base / "fake_gh"
        self.gh.write_text(FAKE_GH)
        self.gh.chmod(0o755)
        environment = mock.patch.dict(os.environ, {
            "FORBIDDEN_TERMS": "placeholder", gate_worktree.SCRATCH_ROOT_ENV: str(self.scratch),
            "FAKE_GH_DIR": str(self.gh_dir), "FAKE_ORIGIN": str(self.origin),
            "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
        })
        environment.start()
        self.addCleanup(environment.stop)
        isolate_environment()
        self.start = self.land("start", {"scripts/check_all.sh": FAKE_GATE, "a": "1"})

    def land(self, name, files):
        sha = commit(self.root, name, files)
        git(self.root, "push", "-q", "origin", "main")
        return sha

    def watch(self, *extra):
        return main_watch.main(["--repo", str(self.root), "--health-dir", str(self.health),
                                "--gh", str(self.gh), *extra])

    def status(self):
        return json.loads((self.health / main_watch.STATUS_FILE).read_text())

    def gh_calls(self):
        path = self.gh_dir / "calls.txt"
        return path.read_text().splitlines() if path.exists() else []

    def origin_branches(self):
        return git(self.origin, "for-each-ref", "--format=%(refname:short)", "refs/heads").split()

    def origin_head(self):
        return git(self.origin, "rev-parse", "refs/heads/main")

    def red_main(self):
        """A green main, then a culprit between two good commits."""
        self.assertEqual(self.watch("--apply"), 0)
        self.land("two", {"b": "2"})
        culprit = self.land("breaks the unit step", {"broken": "x"})
        head = self.land("four", {"c": "3"})
        return culprit, head

    def scripted(self, result, culprit=None, candidates=(), step="unit", exit_code=None, note=None,
                 extra=None):
        """Replace the gate run with the record it would write (post_merge has tests of its own)."""
        def gate(argv):
            sha = argv[argv.index("--sha") + 1]
            health = Path(argv[argv.index("--health-dir") + 1])
            log = health / f"{sha}.log"
            log.write_text("step output\ncheck_all: FAILED step unit (area always, exit 1)\n")
            info = lambda commit: post_merge_records.commit_info(self.root, commit)
            record = {"sha": sha, "result": result, "log": str(log), "failed_step": step if result != "pass" else None}
            if culprit:
                record["culprit"] = info(culprit)
            if candidates:
                record["candidates"] = [info(commit) for commit in candidates]
            if note:
                record["note"] = note
            record.update(extra or {})
            post_merge_records.write_json(health / f"{sha}.json", record)
            return 0 if result == "pass" else 1 if exit_code is None else exit_code

        return mock.patch.object(post_merge, "main", gate)


class RevertTests(WatchCase):
    def test_a_confirmed_culprit_is_reverted_through_a_merged_pull_request(self):
        culprit, head = self.red_main()  # the real gate: post_merge finds the culprit
        self.assertEqual(self.watch("--apply"), main_watch.RED_EXIT)
        title = (self.gh_dir / "title.txt").read_text().strip()
        self.assertEqual(title, "Revert breaks the unit step")
        body = (self.gh_dir / "body.txt").read_text()
        self.assertIn("Failing step: `unit`", body)
        self.assertIn(f"{head}.log", body)
        self.assertIn(culprit, body)
        self.assertIn("FAILED step unit", body)
        message = (self.gh_dir / "commit.txt").read_text()
        self.assertTrue(message.startswith("Revert breaks the unit step\n"))
        self.assertIn(f"This reverts commit {culprit}.", message)
        self.assertIn(f"main {head[:12]} at step unit", message)
        self.assertEqual((self.gh_dir / "changes.txt").read_text().split(), ["D", "broken"])
        calls = self.gh_calls()
        self.assertEqual([call.split()[:2] for call in calls], [["pr", "create"], ["pr", "merge"]])
        self.assertIn("--squash", calls[1])
        self.assertEqual(self.origin_branches(), ["main"], "the revert branch is deleted after the merge")
        git(self.root, "pull", "-q", "origin", "main")
        self.assertEqual(git(self.root, "log", "-1", "--format=%s", "main"), "Revert breaks the unit step (#7)")
        status = self.status()
        self.assertEqual(status["result"], "red-reverted")
        self.assertEqual(status["last_sha"], head)
        self.assertEqual(status["culprit"]["sha"], culprit)
        self.assertEqual(status["revert"]["state"], "merged")
        self.assertEqual(status["revert"]["pr"], "https://example.test/pull/7")
        self.assertEqual(status["failed_step"], "unit")
        self.assertEqual(git(self.root, "branch", "--list", "main-watch/*"), "")
        self.assertEqual(git(self.root, "worktree", "list").count("\n"), 0)
        # The next pass finds the repaired main green.
        self.assertEqual(self.watch("--apply"), 0)
        status = self.status()
        self.assertEqual(status["result"], "green")
        self.assertEqual(status["last_sha"], self.origin_head())
        self.assertEqual(len(status["reverts"]), 1)
        self.assertEqual(self.watch("--apply"), 0)
        self.assertEqual(self.status()["result"], "idle")

    def test_an_ambiguous_culprit_is_not_reverted(self):
        self.assertEqual(self.watch("--apply"), 0)
        self.land("old gate", {"scripts/check_all.sh": FAKE_GATE.replace("--only", "--o_n_l_y")})
        self.land("breaks", {"broken": "x"})
        head = self.land("three", {"c": "3"})
        self.assertEqual(self.watch("--apply"), main_watch.RED_EXIT)
        self.assertEqual(self.gh_calls(), [])
        status = self.status()
        self.assertEqual(status["result"], "red-ambiguous")
        self.assertEqual([c["subject"] for c in status["candidates"]], ["old gate", "breaks", "three"])
        self.assertIsNone(status["revert"])
        self.assertEqual(status["last_sha"], head)

    def test_dry_run_is_the_default_and_changes_nothing(self):
        culprit = self.land("breaks the unit step", {"broken": "x"})
        head = self.land("four", {"c": "3"})
        with self.scripted("fail", culprit):
            self.assertEqual(self.watch(), main_watch.RED_EXIT)
            self.assertEqual(self.gh_calls(), [])
            self.assertEqual(self.origin_branches(), ["main"])
            self.assertEqual(self.origin_head(), head)
            status = self.status()
            self.assertEqual(status["result"], "red-revert-planned")
            self.assertEqual(status["revert"]["state"], "planned")
            self.assertEqual(status["revert"]["culprit"], culprit)
            self.assertTrue(status["dry_run"])
            self.assertNotIn("last_sha", status, "a dry run leaves the pass open")
            self.assertEqual(self.watch("--dry-run"), main_watch.RED_EXIT)
            # The later --apply still acts on the same red commit.
            self.assertEqual(self.watch("--apply"), main_watch.RED_EXIT)
        self.assertEqual(self.status()["result"], "red-reverted")
        self.assertEqual(len(self.gh_calls()), 2)

    def test_apply_and_dry_run_exclude_each_other(self):
        with self.assertRaises(SystemExit):
            self.watch("--apply", "--dry-run")

    def test_a_flaky_step_is_not_reverted(self):
        self.land("breaks", {"broken": "x"})
        with self.scripted("flaky"):
            self.assertEqual(self.watch("--apply"), main_watch.RED_EXIT)
        self.assertEqual(self.gh_calls(), [])
        self.assertEqual(self.status()["result"], "flaky")

    def test_the_status_reports_a_step_with_two_flakes_in_ten_runs_as_chronic(self):
        for number in (1, 2):  # two earlier runs of the ledger, each with the same flaky step
            gate_flakes.record_run(self.health, {
                "sha": f"{number:040x}", "started_at": f"2026-10-0{number}T00:00:00Z",
                "flaky_steps": [gate_flakes.flaky_step("swift:test-shared", {}, None)]})
        self.land("breaks", {"broken": "x"})
        with self.scripted("flaky"):
            self.assertEqual(self.watch("--apply"), main_watch.RED_EXIT)
        chronic = self.status()["chronic"]
        self.assertEqual([(step["step"], step["flakes"]) for step in chronic], [("swift:test-shared", 2)])
        self.assertEqual(self.gh_calls(), [], "a chronic step is reported, not reverted")

    def test_the_summary_names_a_chronic_step_and_says_to_fix_it(self):
        chronic = [{"step": "engine:timing-frame-dense", "flakes": 3, "runs": 10, "shas": []}]
        text = main_watch.summary("flaky", "a" * 40, {"failed_step": "x"}, None, chronic)
        self.assertIn("chronic flake: engine:timing-frame-dense flaked in 3 of the last 10 runs", text)
        self.assertIn("fix it, do not skip it", text)

    def test_flakes_of_two_real_runs_make_the_step_chronic(self):
        self.land("breaks", {"broken": "x"})
        with mock.patch.dict(os.environ, {"FAKE_FLAKY": "1"}):  # every rerun passes: a flake
            self.assertEqual(self.watch("--apply"), main_watch.RED_EXIT)
            self.assertEqual(self.status()["chronic"], [], "one flake is not chronic")
            self.land("two", {"b": "2"})
            self.assertEqual(self.watch("--apply"), main_watch.RED_EXIT)
        chronic = self.status()["chronic"]
        self.assertEqual([(step["step"], step["flakes"], step["runs"]) for step in chronic], [("unit", 2, 2)])

    def test_a_status_without_flakes_has_an_empty_chronic_list(self):
        self.assertEqual(self.watch("--apply"), 0)
        self.assertEqual(self.status()["chronic"], [])

    def test_an_unknown_failing_step_is_not_reverted(self):
        self.land("breaks", {"broken": "x"})
        with self.scripted("fail", note="no known green main to search from"):
            self.assertEqual(self.watch("--apply"), main_watch.RED_EXIT)
        self.assertEqual(self.gh_calls(), [])
        self.assertEqual(self.status()["result"], "red-unknown")

    def test_a_revert_is_never_reverted(self):
        culprit = self.land("Revert the earlier change", {"broken": "x"})
        with self.scripted("fail", culprit):
            self.assertEqual(self.watch("--apply"), main_watch.RED_EXIT)
        self.assertEqual(self.gh_calls(), [])
        status = self.status()
        self.assertEqual(status["result"], "red-not-reverted")
        self.assertIn("is a revert", status["revert"]["reason"])

    def test_a_culprit_that_was_reverted_already_is_left_alone(self):
        culprit = self.land("breaks", {"broken": "x"})
        self.land(f"Revert breaks\n\nThis reverts commit {culprit}", {"b": "2"})
        with self.scripted("fail", culprit):
            self.assertEqual(self.watch("--apply"), main_watch.RED_EXIT)
        self.assertEqual(self.gh_calls(), [])
        self.assertIn("reverted already", self.status()["revert"]["reason"])

    def test_a_culprit_outside_main_is_left_alone(self):
        git(self.root, "checkout", "-q", "-b", "side")
        stray = commit(self.root, "stray", {"s": "1"})
        git(self.root, "checkout", "-q", "main")
        self.land("two", {"b": "2"})
        with self.scripted("fail", stray):
            self.assertEqual(self.watch("--apply"), main_watch.RED_EXIT)
        self.assertEqual(self.gh_calls(), [])
        self.assertIn("not in the history of origin/main", self.status()["revert"]["reason"])

    def test_a_revert_that_does_not_apply_is_reported_and_pushes_nothing(self):
        culprit = self.land("breaks", {"broken": "x", "shared": "one\n"})
        self.land("builds on the culprit", {"shared": "two\n"})
        with self.scripted("fail", culprit):
            self.assertEqual(self.watch("--apply"), main_watch.REVERT_FAILED_EXIT)
        self.assertEqual(self.gh_calls(), [])
        self.assertEqual(self.origin_branches(), ["main"])
        revert = self.status()["revert"]
        self.assertEqual(revert["state"], "failed")
        self.assertIn("does not apply", revert["reason"])
        self.assertEqual(self.status()["result"], "red-revert-failed")
        self.assertEqual(git(self.root, "branch", "--list", "main-watch/*"), "")
        self.assertEqual(git(self.root, "worktree", "list").count("\n"), 0)

    def test_a_pull_request_that_cannot_merge_stays_open_and_is_reported(self):
        culprit = self.land("breaks", {"broken": "x"})
        with self.scripted("fail", culprit), mock.patch.dict(os.environ, {"FAKE_GH_FAIL_MERGE": "1"}):
            self.assertEqual(self.watch("--apply"), main_watch.REVERT_FAILED_EXIT)
        revert = self.status()["revert"]
        self.assertEqual(revert["state"], "open")
        self.assertEqual(revert["pr"], "https://example.test/pull/7")
        self.assertIn("main-watch/revert-", " ".join(self.origin_branches()))
        self.assertEqual(git(self.root, "branch", "--list", "main-watch/*"), "")

    def test_a_merge_commit_is_reverted_against_its_first_parent(self):
        git(self.root, "checkout", "-q", "-b", "side")
        commit(self.root, "side work", {"broken": "x"})
        git(self.root, "checkout", "-q", "main")
        git(self.root, "merge", "-q", "--no-ff", "-m", "Merge the side work", "side")
        git(self.root, "push", "-q", "origin", "main")
        merge = git(self.root, "rev-parse", "HEAD")
        with self.scripted("fail", merge):
            self.assertEqual(self.watch("--apply"), main_watch.RED_EXIT)
        self.assertEqual(self.status()["result"], "red-reverted")
        self.assertEqual((self.gh_dir / "changes.txt").read_text().split(), ["D", "broken"])


class MeasurementTests(WatchCase):
    """A missing editor measurement is not a red main: no revert, the pass stays open."""

    def test_a_missing_measurement_reverts_nothing_and_leaves_the_pass_open(self):
        self.land("two", {"b": "2"})
        gate = {"editor_tree_hash": "ab" * 32, "note": "no valid VM1 measurement"}
        with self.scripted("needs-measurement", step="editor-performance:check",
                           exit_code=post_merge_editor.NEEDS_MEASUREMENT_EXIT, extra=gate):
            self.assertEqual(self.watch("--apply"), main_watch.NEEDS_MEASUREMENT_EXIT)
        self.assertEqual(self.gh_calls(), [])
        status = self.status()
        self.assertEqual(status["result"], "needs-measurement")
        self.assertEqual(status["editor_tree_hash"], "ab" * 32)
        self.assertEqual(status["failed_step"], "editor-performance:check")
        self.assertIsNone(status["revert"])
        self.assertNotIn("last_sha", status)
        self.assertNotEqual(main_watch.NEEDS_MEASUREMENT_EXIT, main_watch.RED_EXIT)

    def test_a_missing_measurement_found_by_the_real_gate_starts_no_search(self):
        self.land("editor gate", {"scripts/check_all.sh": FAKE_EDITOR_GATE})
        report = self.base / "report.json"
        head = self.land("three", {"c": "3"})
        self.assertEqual(self.watch("--apply"), main_watch.NEEDS_MEASUREMENT_EXIT)
        status = self.status()
        self.assertEqual(status["result"], "needs-measurement")
        self.assertTrue(status["editor_tree_hash"])
        self.assertIsNone(status["culprit"])
        self.assertEqual(self.gh_calls(), [])
        self.assertEqual(self.origin_head(), head)
        # Measured on VM1 and handed over: the same commit runs again and is green.
        main_watch.write_json(report, {"editor_tree_hash": status["editor_tree_hash"]})
        self.assertEqual(self.watch("--apply", "--editor-meter", str(report)), 0)
        self.assertEqual(self.status()["result"], "green")
        self.assertEqual(self.status()["last_sha"], head)

    def test_the_meter_and_the_worktree_go_to_the_gate(self):
        seen = []

        def gate(argv):
            seen.append(argv)
            return post_merge_gate.INFRASTRUCTURE_EXIT

        with mock.patch.object(post_merge, "main", gate):
            self.watch("--editor-meter", "/srv/report.json", "--worktree", "/srv/warm")
        argv = seen[0]
        self.assertEqual(argv[argv.index("--editor-meter") + 1], "/srv/report.json")
        self.assertEqual(argv[argv.index("--worktree") + 1], "/srv/warm")

    def test_the_environment_reaches_the_gate_without_options(self):
        seen = []
        with mock.patch.object(post_merge, "main", lambda argv: seen.append(argv) or 2), \
                mock.patch.dict(os.environ, {post_merge_editor.EDITOR_METER_ENV: "/srv/report.json"}):
            self.watch()
        self.assertIn("/srv/report.json", seen[0])


class PassTests(WatchCase):
    def test_a_green_batch_is_one_gate_run_on_the_newest_commit(self):
        self.assertEqual(self.watch("--apply"), 0)
        middle = self.land("two", {"b": "2"})
        head = self.land("three", {"c": "3"})
        self.assertEqual(self.watch("--apply"), 0)
        status = self.status()
        self.assertEqual((status["result"], status["last_sha"], status["batch"]), ("green", head, 2))
        self.assertTrue((self.health / f"{head}.json").exists())
        self.assertFalse((self.health / f"{middle}.json").exists())

    def test_nothing_new_runs_no_gate(self):
        with self.scripted("pass"):
            self.assertEqual(self.watch("--apply"), 0)
        with mock.patch.object(post_merge, "main", side_effect=AssertionError("ran the gate")):
            self.assertEqual(self.watch("--apply"), 0)
        self.assertEqual(self.status()["result"], "idle")

    def test_a_gate_that_cannot_run_changes_nothing(self):
        with mock.patch.dict(os.environ, {"FAKE_INFRA": "1"}):
            self.assertEqual(self.watch("--apply"), main_watch.INFRASTRUCTURE_EXIT)
        status = self.status()
        self.assertEqual(status["result"], "infrastructure")
        self.assertNotIn("last_sha", status)
        self.assertEqual(self.gh_calls(), [])

    def test_only_one_pass_at_a_time(self):
        self.health.mkdir(parents=True)
        with (self.health / main_watch.LOCK_FILE).open("w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            self.assertEqual(self.watch("--apply"), post_merge.BUSY_EXIT)
        with self.scripted("pass"):
            self.assertEqual(self.watch("--apply"), 0)

    def test_a_busy_gate_leaves_the_pass_open(self):
        with mock.patch.object(post_merge, "main", return_value=post_merge.BUSY_EXIT):
            self.assertEqual(self.watch("--apply"), post_merge.BUSY_EXIT)
        self.assertFalse((self.health / main_watch.STATUS_FILE).exists())

    def test_the_remote_host_and_the_starting_point_go_to_the_gate(self):
        seen = []

        def gate(argv):
            seen.append(argv)
            return post_merge_gate.INFRASTRUCTURE_EXIT

        with mock.patch.object(post_merge, "main", gate):
            self.watch("--remote", "linux-host", "--remote-root", "/srv/soyeht", "--since", self.start)
        argv = seen[0]
        self.assertEqual(argv[argv.index("--remote") + 1], "linux-host")
        self.assertEqual(argv[argv.index("--remote-root") + 1], "/srv/soyeht")
        self.assertEqual(argv[argv.index("--since") + 1], self.start)
        self.assertEqual(argv[argv.index("--sha") + 1], self.origin_head())

    def test_remote_needs_a_root(self):
        with self.assertRaises(SystemExit):
            self.watch("--remote", "linux-host")

    def test_history_of_reverts_is_bounded(self):
        history = [{"culprit": str(i)} for i in range(main_watch.HISTORY_LIMIT + 5)]
        main_watch.write_json(self.health / main_watch.STATUS_FILE, {"reverts": history})
        culprit = self.land("breaks", {"broken": "x"})
        with self.scripted("fail", culprit):
            self.assertEqual(self.watch("--apply"), main_watch.RED_EXIT)
        reverts = self.status()["reverts"]
        self.assertEqual(len(reverts), main_watch.HISTORY_LIMIT)
        self.assertEqual(reverts[-1]["culprit"], culprit)


class HelperTests(unittest.TestCase):
    def test_the_title_is_revert_and_the_subject(self):
        self.assertEqual(main_watch.revert_title("Fix the thing (#12)"), "Revert Fix the thing (#12)")

    def test_records_are_classified(self):
        self.assertEqual(main_watch.classify({"result": "pass"}), "green")
        self.assertEqual(main_watch.classify({"result": "flaky", "failed_step": "x"}), "flaky")
        self.assertEqual(main_watch.classify({"result": "fail", "culprit": {"sha": "a"}}), "culprit")
        self.assertEqual(main_watch.classify({"result": "fail", "candidates": [{"sha": "a"}]}), "ambiguous")
        self.assertEqual(main_watch.classify({"result": "fail"}), "unknown")
        self.assertEqual(
            main_watch.classify({"result": "fail", "culprit": {"sha": "a"}, "candidates": [{"sha": "a"}]}),
            "culprit",
        )

    def test_the_log_excerpt_is_the_end_of_the_log(self):
        log = Path(tempfile.mkdtemp()) / "x.log"
        log.write_text("\n".join(f"line {i}" for i in range(500)))
        excerpt = main_watch.log_excerpt({"log": str(log)})
        self.assertTrue(excerpt.endswith("line 499"))
        self.assertLessEqual(len(excerpt), main_watch.LOG_TAIL_CHARS)
        self.assertNotIn("line 100\n", excerpt)
        self.assertEqual(main_watch.log_excerpt({"log": str(log) + ".gone"}), "(the log is not available)")

    def test_the_script_never_forces_a_push_or_touches_other_branches(self):
        source = Path(main_watch.__file__).read_text()
        for forbidden in ("--force-with-lease", "push -f", "push --force", "reset --hard", "checkout main"):
            self.assertNotIn(forbidden, source)
        pushes = [line for line in source.splitlines() if '"push"' in line]
        self.assertEqual(len(pushes), 2)
        self.assertTrue(all("branch" in line for line in pushes), pushes)
        self.assertTrue(main_watch.BRANCH_PREFIX.startswith("main-watch/"))


if __name__ == "__main__":
    unittest.main()
