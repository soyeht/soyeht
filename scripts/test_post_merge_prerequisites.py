"""A rerun of a step on a commit first runs what the step reads from earlier steps of the run."""

import os
import unittest
from unittest import mock

import post_merge_gate
from test_post_merge_fixtures import FAKE_ONLY_ARGUMENTS, FAKE_RECORD_STEP, SyntheticRepositoryCase


# A stand-in with a step `build` that leaves `.built` (not in the commit, so it survives a checkout
# like the engine's target directory does) holding the commit's `version` file, a step `use` that
# fails when `.built` says `bad`, and a step `other` that reads nothing. Run in that order.
FAKE_BUILD_GATE = """#!/bin/bash
# supports --only
""" + FAKE_ONLY_ARGUMENTS + """if ((list)); then printf 'engine both build\nswift both use\nswift both other\\n'; exit 0; fi
[[ -z "${FAKE_TRACE:-}" ]] || echo "$PWD only=$only" >> "$FAKE_TRACE"
steps=""
for id in build use other; do
    [[ -z "$only" || ",$only," == *",$id,"* ]] || continue
    status=0
    if [[ "$id" == build ]]; then cp version .built
    elif [[ "$id" == use ]] && grep -q bad .built; then status=1; fi
""" + FAKE_RECORD_STEP


class PrerequisiteProbeTests(SyntheticRepositoryCase):
    FILES = {"scripts/check_all.sh": FAKE_BUILD_GATE, "version": "good"}

    def setUp(self):
        super().setUp()
        self.trace = self.root.parent / "trace.txt"
        environment = mock.patch.dict(os.environ, {"FAKE_TRACE": str(self.trace)})
        environment.start()
        self.addCleanup(environment.stop)
        self.assertEqual(self.run_main(self.shas[0]), 0)
        self.trace.unlink()

    def selections(self):
        return [line.rsplit("only=", 1)[1] for line in self.trace.read_text().splitlines()]

    def red_main(self):
        """Four commits after the green one; the second makes `use` fail through what `build` leaves."""
        self.add("one", {"a": "1"})
        culprit = self.add("breaks what is built", {"version": "bad"})
        self.add("three", {"b": "2"})
        return culprit, self.add("four", {"c": "3"})

    def test_a_probe_builds_before_the_step_so_the_culprit_is_the_commit_that_breaks_it(self):
        culprit, head = self.red_main()
        with mock.patch.object(post_merge_gate, "PREREQUISITES", {"use": ("build",)}):
            self.assertEqual(self.run_main(head), 1)
        record = self.record(head)
        self.assertEqual(record["failed_step"], "use")
        self.assertEqual(record["culprit"]["sha"], culprit)
        self.assertEqual(record["culprit"]["subject"], "breaks what is built")
        probes = self.selections()[1:]  # the first line is the run itself
        self.assertTrue(probes)
        self.assertEqual(set(probes), {"build,use"})

    def test_without_the_prerequisite_the_stale_programs_are_the_hosts_and_blame_nobody(self):
        # The failure this guards against: every probe reads what the newest commit built. Before
        # the control run it named a commit that is not to blame; now the last green main fails the
        # step too, with the programs the head built, and the result is the host's.
        _, head = self.red_main()
        with mock.patch.object(post_merge_gate, "PREREQUISITES", {}):
            self.assertEqual(self.run_main(head), 1)
        record = self.record(head)
        self.assertEqual(record["result"], "host")
        self.assertNotIn("culprit", record)

    def test_a_step_without_prerequisites_is_probed_alone(self):
        self.assertEqual(post_merge_gate.step_with_prerequisites("other"), "other")
        with mock.patch.object(post_merge_gate, "PREREQUISITES", {"use": ("build",)}):
            self.assertEqual(post_merge_gate.step_with_prerequisites("use"), "build,use")


class PrerequisiteTableTests(unittest.TestCase):
    def test_a_step_takes_every_prerequisite_in_order_before_itself(self):
        with mock.patch.object(post_merge_gate, "PREREQUISITES", {"x": ("p", "q")}):
            self.assertEqual(post_merge_gate.step_with_prerequisites("x"), "p,q,x")

    def test_the_suites_and_the_swift_tests_take_the_build_of_the_programs_they_start(self):
        self.assertEqual(post_merge_gate.step_with_prerequisites("e2e:core"),
                         "engine:build-product,engine:build-test-engine,e2e:core")
        self.assertEqual(post_merge_gate.step_with_prerequisites("swift:test-shared"),
                         "swift:engine-binaries,swift:test-shared")
        self.assertEqual(post_merge_gate.step_with_prerequisites("e2e:tcc-check"),
                         "e2e:tcc-snapshot,e2e:tcc-check")


if __name__ == "__main__":
    unittest.main()
