"""Regression tests for the versioned Q1 quality ratchet."""

from __future__ import annotations

import copy
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from quality_core import (
    DUPLICATION_SCANS,
    JUSTIFICATIONS_NAME,
    MEASUREMENT_VERSION,
    PANIC_KEY,
    SWALLOWED_KEY,
    QualityError,
    baseline_from_measurement,
    baseline_relaxations,
    duplicate_measurement,
    differences,
    engine_production_panics,
    measure,
    periphery_status,
    source_measurements,
    tool_paths,
    unjustified_relaxations,
)
from review_report import render


ROOT = Path(__file__).resolve().parents[1]
REPEATED_RUST = (
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


def git(root: Path, *arguments: str) -> None:
    subprocess.run(
        [
            "git",
            "-c",
            "user.name=Gate Test",
            "-c",
            "user.email=gate@example.invalid",
            "-c",
            "commit.gpgsign=false",
            *arguments,
        ],
        cwd=root,
        check=True,
        capture_output=True,
    )


def publish_as_origin_main(root: Path) -> None:
    """Commit the tree and point origin/main at it, as a merged main would."""
    if not (root / ".git").exists():
        git(root, "init", "--quiet")
    git(root, "add", "--all")
    git(root, "commit", "--quiet", "--allow-empty", "--message", "Base")
    git(root, "update-ref", "refs/remotes/origin/main", "HEAD")


def empty_measurement() -> dict:
    return {
        "version": MEASUREMENT_VERSION,
        PANIC_KEY: 0,
        SWALLOWED_KEY: dict.fromkeys(
            ("swift_try_optional", "rust_ok", "rust_let_underscore"), 0
        ),
        "duplication": {
            area: {"clones": 0, "duplicated_lines": 0, "percent": 0}
            for area in DUPLICATION_SCANS
        },
        "oversized_files": {},
        "loose_literals": {},
    }


class QualityRatchetTests(unittest.TestCase):
    def test_clone_increase_is_red_and_decrease_requires_baseline_update(self):
        baseline = empty_measurement()
        actual = copy.deepcopy(baseline)
        actual["duplication"]["engine"]["clones"] = 1
        self.assertEqual(differences(baseline, actual)[0][0], "RELAXATION")
        self.assertEqual(differences(actual, baseline)[0][0], "IMPROVEMENT")

    def test_an_area_the_older_baseline_did_not_scan_has_nothing_to_compare(self):
        after = empty_measurement()
        before = copy.deepcopy(after)
        del before["duplication"]["desktop"]
        after["duplication"]["desktop"]["clones"] = 3
        self.assertEqual(baseline_relaxations(before, after), [])

    def test_percentage_is_informational_not_in_the_baseline(self):
        measured = empty_measurement()
        baseline = baseline_from_measurement(measured)
        self.assertNotIn("percent", baseline["duplication"]["engine"])
        measured["duplication"]["engine"]["percent"] = 1.23
        self.assertEqual(differences(baseline, measured), [])

    def test_literal_trade_between_files_is_red(self):
        baseline = empty_measurement()
        old_file = "apps/mac/Sources/Old.swift"
        new_file = "apps/mac/Sources/AppRail.swift"
        baseline["loose_literals"][old_file] = {"padding": 1}
        actual = copy.deepcopy(baseline)
        actual["loose_literals"] = {new_file: {"padding": 1}}
        changes = differences(baseline, actual)
        self.assertEqual(
            [change.direction for change in changes], ["RELAXATION", "IMPROVEMENT"]
        )

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
        actual["loose_literals"]["apps/mac/Sources/New.swift"] = {"padding": 1}
        self.assertEqual(
            [change.direction for change in differences(baseline, actual)],
            ["RELAXATION", "RELAXATION"],
        )

    def test_any_engine_production_panic_fails_whatever_the_baseline_says(self):
        baseline = empty_measurement()
        actual = copy.deepcopy(baseline)
        actual[PANIC_KEY] = 1
        (change,) = differences(baseline, actual)
        self.assertEqual((change.direction, change.key), ("RELAXATION", PANIC_KEY))
        baseline[PANIC_KEY] = 5  # an edited baseline cannot buy headroom
        self.assertEqual([c.key for c in differences(baseline, actual)], [PANIC_KEY])
        reason = {PANIC_KEY: {"value": 1, "reason": "no reason is enough"}}
        self.assertEqual(
            [c.key for c in unjustified_relaxations(empty_measurement(), actual, reason)],
            [PANIC_KEY],
        )
        actual[PANIC_KEY] = 0
        self.assertEqual(differences(baseline, actual), [])

    def test_engine_panics_skip_test_items_tests_dirs_and_generated_code(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "engine/demo/src"
            source.mkdir(parents=True)
            (source / "lib.rs").write_text(
                "fn live(v: Option<u8>) -> u8 { v.unwrap() }\n"
                "fn loud() { panic!(\"stop\") }\n"
                "// value.expect(\"in a comment\")\n"
                "#[cfg(test)]\nmod tests { fn t() { None::<u8>.unwrap(); } }\n"
                "#[cfg(all(test, target_os = \"macos\"))]\n"
                "mod mac_tests { fn t() { None::<u8>.unwrap(); } }\n"
            )
            (source / "generated.rs").write_text("fn g() { None::<u8>.unwrap(); }\n")
            for directory_name in ("tests", "benches"):
                extra = root / "engine/demo" / directory_name
                extra.mkdir()
                (extra / "it.rs").write_text("fn t() { None::<u8>.expect(\"x\"); }\n")
            self.assertEqual(engine_production_panics(root), 2)

    def test_desktop_production_code_counts_toward_the_same_panic_ceiling(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "apps/desktop/crates/demo/src"
            source.mkdir(parents=True)
            (source / "lib.rs").write_text(
                "pub fn live(v: Option<u8>) -> u8 { v.unwrap() }\n"
                "#[cfg(test)]\nmod tests { fn t() { None::<u8>.unwrap(); } }\n"
            )
            tests = root / "apps/desktop/crates/demo/tests"
            tests.mkdir()
            (tests / "it.rs").write_text("fn t() { None::<u8>.expect(\"x\"); }\n")
            self.assertEqual(engine_production_panics(root), 1)

    def test_engine_panics_skip_test_modules_in_their_own_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "engine/demo/src"
            (source / "push").mkdir(parents=True)
            (source / "lib.rs").write_text(
                "mod push;\nmod live;\n#[cfg(test)]\nmod foo;\n"
                "#[cfg(all(test, target_os = \"macos\"))]\npub(crate) mod mac_fixture;\n"
                "pub fn after_declaration(v: Option<u8>) -> u8 { v.expect(\"counted\") }\n"
            )
            (source / "foo.rs").write_text(
                "mod helpers;\nfn t() { None::<u8>.unwrap(); }\n"
            )
            (source / "foo").mkdir()
            (source / "foo/helpers.rs").write_text("fn h() { panic!(\"test only\") }\n")
            (source / "mac_fixture.rs").write_text("fn f() { None::<u8>.unwrap(); }\n")
            (source / "push/mod.rs").write_text("#[cfg(test)]\nmod tests;\n")
            (source / "push/tests.rs").write_text("fn t() { None::<u8>.unwrap(); }\n")
            (source / "live.rs").write_text("pub fn live(v: Option<u8>) -> u8 { v.unwrap() }\n")
            self.assertEqual(engine_production_panics(root), 2)

    def test_raising_a_baseline_entry_needs_a_reason_for_that_value(self):
        previous = empty_measurement()
        raised = copy.deepcopy(previous)
        raised["oversized_files"]["engine/big.rs"] = 900
        key = "oversized_files.engine/big.rs"
        self.assertEqual(
            [change.key for change in unjustified_relaxations(previous, raised, {})],
            [key],
        )
        stale = {key: {"value": 850, "reason": "an older, smaller raise"}}
        self.assertEqual(len(unjustified_relaxations(previous, raised, stale)), 1)
        blank = {key: {"value": 900, "reason": "  "}}
        self.assertEqual(len(unjustified_relaxations(previous, raised, blank)), 1)
        written = {key: {"value": 900, "reason": "generated table split next"}}
        self.assertEqual(unjustified_relaxations(previous, raised, written), [])
        # Lowering an entry never needs a reason, and a missing base checks nothing.
        self.assertEqual(unjustified_relaxations(raised, previous, {}), [])
        self.assertEqual(unjustified_relaxations(None, raised, {}), [])

    def test_report_marks_every_baseline_relaxation_red(self):
        before = empty_measurement()
        before["oversized_files"]["engine/existing.rs"] = 900
        after = copy.deepcopy(before)
        after["duplication"]["engine"]["clones"] = 1
        after["oversized_files"]["engine/existing.rs"] = 901
        after["oversized_files"]["engine/new.rs"] = 810
        after["loose_literals"]["apps/mac/Sources/New.swift"] = {"font_name": 1}
        reasons = {
            "oversized_files.engine/new.rs": {"value": 810, "reason": "split in #999"}
        }
        report = render(
            before, after, after, "main", "SKIPPED (not installed)", None, reasons
        )
        self.assertEqual(report.count("🔴 "), 5)  # heading plus four loosened entries
        self.assertIn("written justification required", report)
        self.assertIn(
            "engine/new.rs: cap 800, actual 810; split the file — "
            "justification: split in #999",
            report,
        )
        self.assertEqual(report.count("MISSING justification"), 3)

    def test_missing_jscpd_has_install_command(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(
                QualityError, r"npm ci --prefix scripts/quality"
            ):
                tool_paths(Path(directory))

    def test_missing_jscpd_is_restored_from_the_lockfile_without_network(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tools = root / "scripts/quality"
            tools.mkdir(parents=True)
            for name in ("package.json", "package-lock.json"):
                shutil.copy(ROOT / "scripts/quality" / name, tools / name)
            jscpd, _ = tool_paths(root)
            self.assertTrue(jscpd.is_file())

    def test_missing_machete_is_red_and_missing_periphery_is_skipped(self):
        with tempfile.TemporaryDirectory() as empty_cargo_home:
            with (
                patch("quality_core.shutil.which", return_value=None),
                patch.dict(os.environ, {"CARGO_HOME": empty_cargo_home}),
            ):
                with self.assertRaisesRegex(
                    QualityError, r"cargo install cargo-machete --version"
                ):
                    tool_paths(ROOT)
                self.assertIn("SKIPPED", periphery_status(ROOT))

    def test_real_clone_is_detected_in_disposable_sources(self):
        jscpd, _ = tool_paths(ROOT)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "engine/crate/src"
            source.mkdir(parents=True)
            (source / "one.rs").write_text(REPEATED_RUST)
            (source / "two.rs").write_text(REPEATED_RUST)
            actual = duplicate_measurement(root, "engine", jscpd)
        self.assertGreater(actual["clones"], 0)

    def test_the_desktop_views_scan_finds_a_clone_the_default_size_misses(self):
        jscpd, _ = tool_paths(ROOT)
        short = (
            "fn chip(label: &str) -> usize {\n"
            "    let first = label.len() + 11;\n"
            "    let second = first * 2 + label.len();\n"
            "    second + first\n"
            "}\n"
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "apps/desktop/crates/soyeht-desktop/src"
            source.mkdir(parents=True)
            (source / "one.rs").write_text(short)
            (source / "two.rs").write_text(short)
            views = duplicate_measurement(root, "desktop_views", jscpd)
            default = duplicate_measurement(root, "desktop", jscpd)
        self.assertGreater(views["clones"], 0)
        self.assertEqual(default["clones"], 0)

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
            literals["apps/mac/Sources/Control.swift"],
            {
                "padding": 1,
                "corner_radius": 1,
                "system_font_size": 1,
                "font_name": 1,
            },
        )
        self.assertEqual(literals["engine/crate/src/mod.rs"]["rust_limits"], 1)

    def test_full_gate_fails_regressions(self):
        jscpd, machete = tool_paths(ROOT)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = GateFixture(root, jscpd, machete)
            crate, mac_item, rail = fixture.crate, fixture.mac_item, fixture.rail
            self.assertEqual(fixture.gate().returncode, 0)
            clone = crate / "src/clone.rs"
            clone.write_text(REPEATED_RUST)
            clone_result = fixture.gate()
            self.assertEqual(clone_result.returncode, 1)
            self.assertIn("duplication engine.clones", clone_result.stderr)
            print(first_line_with(clone_result.stderr, "duplication engine.clones"))
            clone.unlink()
            mac_item.write_text("let value = 1\n" * 801)
            inflated_result = fixture.gate()
            self.assertEqual(inflated_result.returncode, 1)
            self.assertIn(
                "file apps/mac/Sources/Item.swift: cap 800, actual 801",
                inflated_result.stderr,
            )
            print(
                first_line_with(
                    inflated_result.stderr, "file apps/mac/Sources/Item.swift"
                )
            )
            mac_item.write_text(
                MAC_ITEM.replace(".padding(12)", ".padding(MacTheme.Space.small)")
            )
            rail.write_text(RAIL + ".padding(8)\n")
            traded = fixture.gate()
            self.assertEqual(traded.returncode, 1)
            self.assertIn(
                "literal apps/mac/Sources/AppRail.swift padding", traded.stderr
            )
            print(
                first_line_with(
                    traded.stderr, "literal apps/mac/Sources/AppRail.swift padding"
                )
            )
            mac_item.write_text(MAC_ITEM)
            rail.write_text(RAIL)
            unique = root / "apps/mac/Sources/Unique.swift"
            unique.write_text(
                "struct TurquoiseOrbit {\n"
                '  let nebula = "distant cloud"\n'
                '  let compass = "northern route"\n'
                '  let harbor = "quiet landing"\n'
                '  let lantern = "winter beacon"\n'
                "}\n"
            )
            self.assertEqual(fixture.gate().returncode, 0)

            # A new unwrap in engine production code cannot pass.
            risky = crate / "src/risky.rs"
            risky.write_text("pub fn risky(v: Option<u8>) -> u8 {\n    v.unwrap()\n}\n")
            panicked = fixture.gate()
            self.assertEqual(panicked.returncode, 1)
            self.assertIn(
                "REGRESSION: engine production unwrap/expect/panic!: allowed 0, actual 1",
                panicked.stderr,
            )
            risky.unlink()
            self.assertEqual(fixture.gate().returncode, 0)

    def test_full_gate_needs_exact_baseline_and_justified_raises(self):
        jscpd, machete = tool_paths(ROOT)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = GateFixture(root, jscpd, machete)

            def justify(value: int) -> None:
                (root / JUSTIFICATIONS_NAME).write_text(
                    json.dumps(
                        {
                            "oversized_files.apps/mac/Sources/Big.swift": {
                                "value": value,
                                "reason": "fixture table, split tracked separately",
                            }
                        }
                    )
                )

            # Raising the baseline over origin/main needs a reason for that value.
            big = root / "apps/mac/Sources/Big.swift"
            big.write_text("".join(f"let distinct{i} = {i}\n" for i in range(900)))
            fixture.refresh_baseline()
            raised = fixture.gate()
            self.assertEqual(raised.returncode, 1)
            self.assertIn("UNJUSTIFIED BASELINE RAISE vs", raised.stderr)
            self.assertIn('"oversized_files.apps/mac/Sources/Big.swift"', raised.stderr)
            justify(900)
            self.assertEqual(fixture.gate().returncode, 0)

            # An improvement fails until the baseline is lowered in the same change.
            big.write_text("".join(f"let distinct{i} = {i}\n" for i in range(850)))
            fixture.mac_item.write_text(MAC_ITEM.replace(".padding(12)\n", ""))
            improved = fixture.gate()
            self.assertEqual(improved.returncode, 1)
            self.assertIn(
                "UNLOCKED IMPROVEMENT: file apps/mac/Sources/Big.swift: cap 900, actual 850",
                improved.stderr,
            )
            self.assertIn(
                "UNLOCKED IMPROVEMENT: literal apps/mac/Sources/Item.swift padding",
                improved.stderr,
            )
            self.assertIn(
                "uv run python scripts/check_quality.py --print-baseline > "
                "scripts/quality_baseline.json",
                improved.stderr,
            )
            print(first_line_with(improved.stderr, "Big.swift"))
            fixture.refresh_baseline()
            justify(850)
            self.assertEqual(fixture.gate().returncode, 0)
            big.write_text("".join(f"let distinct{i} = {i}\n" for i in range(901)))
            regressed = fixture.gate()
            self.assertEqual(regressed.returncode, 1)
            self.assertIn(
                "REGRESSION: file apps/mac/Sources/Big.swift: cap 850, actual 901",
                regressed.stderr,
            )

    def test_unresolvable_base_fails_loudly(self):
        with tempfile.TemporaryDirectory() as directory:
            result = run_gate(Path(directory))
        self.assertEqual(result.returncode, 1)
        self.assertIn("cannot resolve the merge-base of origin/main", result.stderr)


def swift_item(prefix: str) -> str:
    return (
        f"struct {prefix}Item {{\n"
        "  let one = 1\n  let two = 2\n  let three = 3\n"
        "  let four = 4\n  let five = 5\n}\n"
    )


MAC_ITEM = swift_item("Mac") + ".padding(12)\n"
RAIL = (
    "struct AppRailFixture {\n"
    '  let graphite = "graphite"\n'
    '  let amber = "amber"\n'
    '  let indigo = "indigo"\n'
    '  let violet = "violet"\n'
    "}\n"
)


def first_line_with(text: str, fragment: str) -> str:
    return next(line for line in text.splitlines() if fragment in line)


def run_gate(root: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(ROOT / "scripts/check_quality.py"), "--root", str(root)],
        capture_output=True,
        text=True,
        timeout=600,
    )


class GateFixture:
    """A disposable repository whose measured baseline is merged on origin/main."""

    def __init__(self, root: Path, jscpd: Path, machete: str):
        self.root, self.jscpd, self.machete = root, jscpd, machete
        bin_dir = root / "scripts/quality/node_modules/.bin"
        bin_dir.mkdir(parents=True)
        (bin_dir / "jscpd").symlink_to(jscpd.resolve())
        self.crate = root / "engine/demo"
        (self.crate / "src").mkdir(parents=True)
        (root / "engine/Cargo.toml").write_text('[workspace]\nmembers = ["demo"]\n')
        (self.crate / "Cargo.toml").write_text(
            '[package]\nname = "demo"\nversion = "0.1.0"\nedition = "2021"\n'
        )
        (self.crate / "src/lib.rs").write_text(REPEATED_RUST)
        desktop = root / "apps/desktop/crates/demo"
        (desktop / "src").mkdir(parents=True)
        (root / "apps/desktop/Cargo.toml").write_text(
            '[workspace]\nmembers = ["crates/demo"]\n'
        )
        (desktop / "Cargo.toml").write_text(
            '[package]\nname = "demo"\nversion = "0.1.0"\nedition = "2021"\n'
        )
        (desktop / "src/lib.rs").write_text(REPEATED_RUST)
        for area, relative in (
            ("shared", "apps/shared/Sources/SoyehtClient/Item.swift"),
            ("dictation", "apps/dictation/Sources/SoyehtDictation/Item.swift"),
            ("ios", "apps/ios/Sources/Item.swift"),
        ):
            source = root / relative
            source.parent.mkdir(parents=True)
            source.write_text(swift_item(area.title()))
        self.mac_item = root / "apps/mac/Sources/Item.swift"
        self.mac_item.parent.mkdir(parents=True)
        self.mac_item.write_text(MAC_ITEM)
        self.rail = root / "apps/mac/Sources/AppRail.swift"
        self.rail.write_text(RAIL)
        self.refresh_baseline()
        publish_as_origin_main(root)

    def refresh_baseline(self) -> None:
        measured = measure(self.root, self.jscpd, self.machete)
        (self.root / "scripts/quality_baseline.json").write_text(
            json.dumps(baseline_from_measurement(measured))
        )

    def gate(self) -> subprocess.CompletedProcess[str]:
        return run_gate(self.root)




if __name__ == "__main__":
    unittest.main()
