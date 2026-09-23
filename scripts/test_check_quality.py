"""Regression tests for the versioned Q1 quality ratchet."""

from __future__ import annotations

import copy
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from quality_core import (
    AREAS,
    DUPLICATION_SCANS,
    QualityError,
    duplicate_measurement,
    differences,
    measure,
    periphery_status,
    source_measurements,
    tool_paths,
)
from review_report import render


ROOT = Path(__file__).resolve().parents[1]


def empty_measurement() -> dict:
    return {
        "version": 1,
        "duplication": {
            area: {"clones": 0, "duplicated_lines": 0, "percent": 0}
            for area in DUPLICATION_SCANS
        },
        "oversized_files": {},
        "loose_literals": {
            "engine": {"rust_limits": 0},
            **{
                area: {
                    "padding": 0,
                    "corner_radius": 0,
                    "system_font_size": 0,
                    "font_name": 0,
                }
                for area in ("shared", "mac", "ios")
            },
        },
    }


class QualityRatchetTests(unittest.TestCase):
    def test_clone_increase_is_red_and_decrease_requires_baseline_update(self):
        baseline = empty_measurement()
        actual = copy.deepcopy(baseline)
        actual["duplication"]["engine"]["clones"] = 1
        self.assertEqual(differences(baseline, actual)[0][0], "RELAXATION")
        self.assertEqual(differences(actual, baseline)[0][0], "IMPROVEMENT")

    def test_existing_file_cap_cannot_grow_and_must_follow_shrinking_file(self):
        baseline = empty_measurement()
        baseline["oversized_files"]["engine/example.rs"] = 900
        actual = copy.deepcopy(baseline)
        actual["oversized_files"]["engine/example.rs"] = 901
        self.assertEqual(differences(baseline, actual)[0][0], "RELAXATION")
        actual["oversized_files"]["engine/example.rs"] = 850
        self.assertEqual(differences(baseline, actual)[0][0], "IMPROVEMENT")
        actual["oversized_files"].clear()
        self.assertEqual(differences(baseline, actual)[0][0], "IMPROVEMENT")

    def test_new_oversized_file_and_loose_literal_fail(self):
        baseline = empty_measurement()
        actual = copy.deepcopy(baseline)
        actual["oversized_files"]["apps/mac/Sources/New.swift"] = 801
        actual["loose_literals"]["mac"]["padding"] = 1
        self.assertEqual(
            [direction for direction, _ in differences(baseline, actual)],
            ["RELAXATION", "RELAXATION"],
        )

    def test_report_marks_every_baseline_relaxation_red(self):
        before = empty_measurement()
        before["oversized_files"]["engine/existing.rs"] = 900
        after = copy.deepcopy(before)
        after["duplication"]["engine"]["percent"] = 0.1
        after["oversized_files"]["engine/existing.rs"] = 901
        after["oversized_files"]["engine/new.rs"] = 810
        after["loose_literals"]["mac"]["font_name"] = 1
        report = render(before, after, after, "main", "SKIPPED (not installed)")
        self.assertEqual(report.count("🔴 "), 5)  # heading plus four loosened entries
        self.assertIn("written justification required", report)

    def test_missing_jscpd_has_install_command(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(
                QualityError, r"npm ci --prefix scripts/quality"
            ):
                tool_paths(Path(directory))

    def test_missing_machete_is_red_and_missing_periphery_is_skipped(self):
        with patch("quality_core.shutil.which", return_value=None):
            with self.assertRaisesRegex(QualityError, r"cargo install cargo-machete"):
                tool_paths(ROOT)
            self.assertIn("SKIPPED", periphery_status(ROOT))

    def test_real_clone_is_detected_in_disposable_sources(self):
        jscpd, _ = tool_paths(ROOT)
        repeated = (
            "fn duplicate(value: usize) -> usize {\n"
            "    let first = value + 11;\n"
            "    let second = first * 2;\n"
            "    let third = second + first;\n"
            "    let fourth = third / 3;\n"
            "    let fifth = fourth + second;\n"
            "    let sixth = fifth + third;\n"
            "    sixth + fourth\n"
            "}\n"
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "engine/crate/src"
            source.mkdir(parents=True)
            (source / "one.rs").write_text(repeated)
            (source / "two.rs").write_text(repeated)
            actual = duplicate_measurement(root, "engine", jscpd)
        self.assertGreater(actual["clones"], 0)

    def test_disposable_file_at_801_lines_is_reported(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "apps/mac/Sources"
            source.mkdir(parents=True)
            (source / "Inflated.swift").write_text("let value = 1\n" * 801)
            oversized, _ = source_measurements(root)
        self.assertEqual(oversized["apps/mac/Sources/Inflated.swift"], 801)

    def test_literal_patterns_find_direct_values_but_exempt_limits(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            swift = root / "apps/mac/Sources/Control.swift"
            swift.parent.mkdir(parents=True)
            swift.write_text(
                ".padding(.horizontal, 8)\n"
                "RoundedRectangle(cornerRadius: 10)\n"
                ".font(.system(size: 14))\n"
                '.font(.custom("Another Font", size: 14))\n'
            )
            rust = root / "engine/crate/src/mod.rs"
            rust.parent.mkdir(parents=True)
            rust.write_text("const BUFFER: usize = 4096;\n")
            (rust.parent / "limits.rs").write_text("const BUFFER: usize = 512;\n")
            _, literals = source_measurements(root)
        self.assertEqual(
            literals["mac"],
            {
                "padding": 1,
                "corner_radius": 1,
                "system_font_size": 1,
                "font_name": 1,
            },
        )
        self.assertEqual(literals["engine"]["rust_limits"], 1)

    def test_full_gate_is_red_for_real_clone_and_inflated_file(self):
        jscpd, machete = tool_paths(ROOT)
        repeated = (
            "pub fn calculation(value: usize) -> usize {\n"
            "    let first = value + 11;\n"
            "    let second = first * 2;\n"
            "    let third = second + first;\n"
            "    let fourth = third / 3;\n"
            "    let fifth = fourth + second;\n"
            "    let sixth = fifth + third;\n"
            "    sixth + fourth\n"
            "}\n"
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            bin_dir = root / "scripts/quality/node_modules/.bin"
            bin_dir.mkdir(parents=True)
            (bin_dir / "jscpd").symlink_to(jscpd.resolve())
            crate = root / "engine/demo"
            (crate / "src").mkdir(parents=True)
            (root / "engine/Cargo.toml").write_text('[workspace]\nmembers = ["demo"]\n')
            (crate / "Cargo.toml").write_text(
                '[package]\nname = "demo"\nversion = "0.1.0"\nedition = "2021"\n'
            )
            (crate / "src/lib.rs").write_text(repeated)
            for area, (_, language, _) in AREAS.items():
                if language == "swift":
                    source = (
                        root
                        / {
                            "shared": "apps/shared/Sources/SoyehtClient/Item.swift",
                            "mac": "apps/mac/Sources/Item.swift",
                            "ios": "apps/ios/Sources/Item.swift",
                        }[area]
                    )
                    source.parent.mkdir(parents=True)
                    source.write_text(
                        f"struct {area.title()}Item {{\n"
                        "  let one = 1\n  let two = 2\n  let three = 3\n"
                        "  let four = 4\n  let five = 5\n}\n"
                    )
            baseline = measure(root, jscpd, machete)
            (root / "scripts/quality_baseline.json").write_text(json.dumps(baseline))

            def gate() -> subprocess.CompletedProcess[str]:
                return subprocess.run(
                    [
                        "python3",
                        str(ROOT / "scripts/check_quality.py"),
                        "--root",
                        str(root),
                    ],
                    capture_output=True,
                    text=True,
                    timeout=30,
                )

            self.assertEqual(gate().returncode, 0)
            clone = crate / "src/clone.rs"
            clone.write_text(repeated)
            clone_result = gate()
            self.assertEqual(clone_result.returncode, 1)
            self.assertIn("duplication engine.clones", clone_result.stderr)
            print(
                next(
                    line
                    for line in clone_result.stderr.splitlines()
                    if "duplication engine.clones" in line
                )
            )
            clone.unlink()
            inflated = root / "apps/mac/Sources/Item.swift"
            inflated.write_text("let value = 1\n" * 801)
            inflated_result = gate()
            self.assertEqual(inflated_result.returncode, 1)
            self.assertIn(
                "file apps/mac/Sources/Item.swift: cap 800, actual 801",
                inflated_result.stderr,
            )
            print(
                next(
                    line
                    for line in inflated_result.stderr.splitlines()
                    if "file apps/mac/Sources/Item.swift" in line
                )
            )


if __name__ == "__main__":
    unittest.main()
