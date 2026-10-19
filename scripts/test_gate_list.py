"""scripts/check_all.sh is the only list of merge-gate steps."""

from __future__ import annotations

import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

import gate_plan
import post_merge


ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = ROOT / ".github/workflows"
GATE_CALL = re.compile(r"\bbash\s+scripts/check_all\.sh\b")
# Commands that are gate steps; a workflow running one keeps a second list.
OWN_STEPS = re.compile(
    r"\bunittest\b"
    r"|\bcargo\s+(?:fmt|clippy|build|test)\b"
    r"|\bswift\s+(?:format|test)\b"
    r"|\bnpm\s+(?:run|test)\b"
    r"|\bscripts/(?!check_all\.sh\b)[\w./-]+\.(?:py|sh)\b"
    r"|\bcontracts/generate\.py\b"
)


def gate_list_problems(name: str, text: str) -> list[str]:
    commands = "\n".join(
        line for line in text.splitlines() if not line.lstrip().startswith("#")
    )
    problems = [
        f"{name}: runs its own gate step `{found.group()}`; add it to scripts/check_all.sh"
        for found in OWN_STEPS.finditer(commands)
    ]
    if not GATE_CALL.search(commands):
        problems.append(f"{name}: does not run bash scripts/check_all.sh")
    return problems


STEP = re.compile(r"^\s*step\s+(\S+)\s+(\S+)\s+(\S+)", re.MULTILINE)
# A fast step that stands in for a full one is named `<full id>-changed`, `-fast` or `-build`.
STAND_IN = re.compile(r"^(?P<full>.+)-(?:changed|fast|build)$")
# The steps that only the full tier (after the merge) runs, whichever area they belong to.
AFTER_MERGE = (
    "engine:test", "desktop:test", "swift:test-shared", "swift:test-dictation", "swift:engine-binaries",
    "mac-app:build", "ios-app:build", "swift:check-swift", "editor-performance:check",
    "editor-performance:rust",
)


def gate_steps() -> list[tuple[str, str, str]]:
    """(area, tiers, id) of every `step` line of check_all.sh."""
    return STEP.findall((ROOT / "scripts/check_all.sh").read_text())


class StepListTests(unittest.TestCase):
    def test_areas_and_tiers_are_known(self):
        steps = gate_steps()
        self.assertGreater(len(steps), 60)
        for area, tiers, step_id in steps:
            with self.subTest(step=step_id):
                self.assertIn(area, gate_plan.ALL_AREAS)
                self.assertIn(tiers, {"both", "full", "fast"})

    def test_ids_are_unique(self):
        ids = [step_id for _, _, step_id in gate_steps()]
        self.assertEqual(sorted(set(ids)), sorted(ids))

    def test_every_fast_step_stands_in_for_a_full_one(self):
        steps = {step_id: (area, tiers) for area, tiers, step_id in gate_steps()}
        for step_id, (area, tiers) in steps.items():
            if tiers != "fast":
                continue
            with self.subTest(step=step_id):
                match = STAND_IN.match(step_id)
                self.assertIsNotNone(match, f"{step_id}: name it <full id>-changed or <full id>-fast")
                self.assertEqual(steps.get(match["full"]), (area, "full"))

    def test_the_heavy_steps_run_only_after_the_merge(self):
        steps = {step_id: (area, tiers) for area, tiers, step_id in gate_steps()}
        for step_id in AFTER_MERGE:
            with self.subTest(step=step_id):
                self.assertEqual(steps[step_id][1], "full")
        for step_id, (area, tiers) in steps.items():
            with self.subTest(step=step_id):
                if area in gate_plan.FULL_ONLY_AREAS:
                    self.assertEqual(tiers, "full")

    def test_the_fast_tier_runs_unit_tests_only(self):
        commands = (ROOT / "scripts/check_all.sh").read_text()
        engine = re.search(r"step engine fast engine:test-changed .*", commands)[0]
        desktop = re.search(r"step desktop fast desktop:test-fast .*", commands)[0]
        for line in (engine, desktop):
            self.assertIn("--lib --bins", line)
        self.assertIn("swift build --package-path apps/shared", commands)
        self.assertNotRegex(commands, r"step \w+ (?:both|fast) \S+ .*swift test")

    def test_post_merge_prerequisites_are_steps_of_the_same_area(self):
        steps = {step_id: area for area, _, step_id in gate_steps()}
        for step_id, prerequisite in post_merge.PREREQUISITES.items():
            with self.subTest(step=step_id):
                self.assertIn(step_id, steps)
                self.assertEqual(steps.get(prerequisite), steps[step_id])

    def test_every_area_of_the_plan_has_a_step(self):
        used = {area for area, _, _ in gate_steps()}
        self.assertEqual(used, set(gate_plan.ALL_AREAS))

    def test_the_script_lists_every_step_of_each_tier(self):
        changed = Path(tempfile.mkdtemp()) / "changed.txt"
        changed.write_text("engine/soyeht-web/src/lib.rs\napps/desktop/crates/soyeht-client/src/lib.rs\napps/shared/Sources/A.swift\n")
        for tier in ("fast", "full"):
            with self.subTest(tier=tier):
                result = subprocess.run(
                    ["bash", "scripts/check_all.sh", "--list", "--tier", tier],
                    cwd=ROOT, capture_output=True, text=True,
                    env={**os.environ, "SOYEHT_GATE_FILES_FROM": str(changed)},
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                listed = [line.split()[-1] for line in result.stdout.splitlines()]
                steps = {step_id: (area, tiers) for area, tiers, step_id in gate_steps()}
                self.assertEqual(len(listed), len(set(listed)))
                for step_id in listed:
                    self.assertIn(steps[step_id][1], ("both", tier))
                always = [i for i, (area, tiers) in steps.items() if area == "always"]
                self.assertTrue(set(always) <= set(listed))
                if tier == "full":
                    # Only the steps that need SOYEHT_REAL_ACP=1 are left out.
                    expected = {i for i, (_, tiers) in steps.items() if tiers in ("both", "full")}
                    self.assertEqual(expected - set(listed), {i for i in expected if i.startswith("e2e:real-")})
                    self.assertNotIn("engine:test-changed", listed)
                    self.assertIn("engine:test", listed)
                else:
                    self.assertNotIn("engine:test", listed)
                    self.assertIn("engine:test-changed", listed)
                    self.assertNotIn("desktop:test", listed)
                    self.assertIn("desktop:test-fast", listed)
                    for step_id in AFTER_MERGE:
                        self.assertNotIn(step_id, listed)
                    self.assertIn("swift:test-shared-build", listed)


class GateListTests(unittest.TestCase):
    def test_every_workflow_delegates_to_check_all(self):
        problems = [
            problem
            for path in sorted(WORKFLOWS.glob("*.y*ml"))
            for problem in gate_list_problems(path.name, path.read_text())
        ]
        self.assertEqual(problems, [])

    def test_a_workflow_with_its_own_steps_is_rejected(self):
        drifted = (
            "steps:\n"
            "  - run: cargo test --locked --workspace\n"
            "  - run: uv run python scripts/e2e.py --report r.json\n"
            "  # bash scripts/check_all.sh only in a comment\n"
        )
        problems = gate_list_problems("drifted.yml", drifted)
        self.assertEqual(len(problems), 3)
        self.assertIn("does not run bash scripts/check_all.sh", problems[-1])

    def test_tool_setup_is_not_a_gate_step(self):
        setup = (
            "  - run: npm ci --prefix scripts/quality --ignore-scripts\n"
            "  - run: bash scripts/check_all.sh --base \"$BASE_OID\"\n"
        )
        self.assertEqual(gate_list_problems("ok.yml", setup), [])


if __name__ == "__main__":
    unittest.main()
