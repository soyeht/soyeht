"""scripts/check_all.sh is the only list of merge-gate steps."""

from __future__ import annotations

import re
import unittest
from pathlib import Path


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
