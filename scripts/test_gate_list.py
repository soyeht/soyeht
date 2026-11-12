"""scripts/check_all.sh is the only list of merge-gate steps."""

from __future__ import annotations

import fnmatch
import os
import posixpath
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

import gate_plan
import post_merge_gate
import post_merge_hosts


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
# (`desktop:test` and the e2e steps run before the merge too.)
AFTER_MERGE = (
    "engine:test", "swift:test-shared", "swift:test-dictation", "swift:engine-binaries",
    "mac-app:build", "ios-app:build", "swift:check-swift", "editor-performance:check",
    "editor-performance:rust",
)


# The steps that make up the end-to-end set, in the order they must run: the programs they start
# are built after every `cargo test` of the engine (which relinks target/debug/soyeht-engine with
# the test hooks), and the TCC guard brackets the suites.
E2E_SET = (
    "engine:build-product", "engine:build-test-engine", "e2e:tcc-snapshot", "e2e:core", "e2e:agents",
    "e2e:tasks", "e2e:editor", "e2e:web", "e2e:writing", "e2e:tcc-check",
)
# The unit tests of the gate scripts that only a change to the scripts can break.
GATE_SCRIPT_UNITS = (
    "post-merge:unit", "post-merge-editor:unit", "post-merge-flakes:unit", "post-merge-remote:unit",
    "post-merge-load:unit", "post-merge-waiver:unit", "post-merge-prerequisites:unit",
    "post-merge-control:unit", "e2e-real-harnesses:unit", "main-watch:unit", "quality:unit",
    "gate-list:unit", "gate-timings:unit", "e2e-swift-client:unit",
)
# One sample change per kind: files that `gate_plan.py` maps to areas.
DESKTOP_ONLY = ["apps/desktop/crates/soyeht-client/src/lib.rs"]
ENGINE_CORE = ["engine/soyeht-engine/src/core.rs"]
EDITOR_CRATE = ["engine/soyeht-editor/src/lib.rs"]
DOCS_ONLY = ["docs/adr/0089-fast-gate-before-merge-full-gate-after.md", "AGENTS.md"]
SCRIPTS_ONLY = ["scripts/check_quality.py"]
ANDROID_APP = ["apps/android/app/src/main/kotlin/com/soyeht/next/android/AppModel.kt"]
ANDROID_CORE_STEPS = ("android-core:fmt", "android-core:clippy", "android-core:test")
ANDROID_APP_STEPS = ("android-app:build", "android-app:test")


def listed_steps(tier: str, files: list[str]) -> list[str]:
    """The ids `check_all.sh --list` prints for a change of `files`, in the order they run."""
    changed = Path(tempfile.mkdtemp()) / "changed.txt"
    changed.write_text("\n".join(files) + "\n")
    result = subprocess.run(
        ["bash", "scripts/check_all.sh", "--list", "--tier", tier],
        cwd=ROOT, capture_output=True, text=True,
        env={**os.environ, "SOYEHT_GATE_FILES_FROM": str(changed)},
    )
    assert result.returncode == 0, result.stderr
    return [line.split()[-1] for line in result.stdout.splitlines()]


def engine_cargo_tests() -> list[str]:
    """Ids of the steps of check_all.sh that run `cargo test` in the engine workspace or the
    engine's package, read from the script so that a new one is held to the order too."""
    text = (ROOT / "scripts/check_all.sh").read_text()
    found = []
    for block in re.finditer(r"^step (\w+) \S+ (\S+) (.*(?:\\\n.*)*)", text, re.MULTILINE):
        area, step_id, command = block.groups()
        runs_tests = re.search(r"cargo test|engine_test_changed|capture_test|run_exact_rust_test", command)
        if runs_tests and not command.lstrip().startswith("in_dir apps/desktop") and area in ("engine", "timing"):
            found.append(step_id)
    return found


def gate_steps() -> list[tuple[str, str, str]]:
    """(area, tiers, id) of every `step` line of check_all.sh."""
    return STEP.findall((ROOT / "scripts/check_all.sh").read_text())


STEP_COMMAND = re.compile(r"^\s*step\s+\w+\s+\w+\s+\S+\s+(.*)$", re.MULTILINE)
# What a discover step runs: `-s <directory> -p '<pattern>'`.
DISCOVER_PATTERN = re.compile(r"discover\b[^&|;]*-s\s+(\S+)\s+-p\s+'([^']+)'")
# A test module with no case of its own, which the tests of the gate import: the fixtures of the
# post_merge tests. `unittest discover` finds nothing in it; the modules that import it run it.
HELPER_TEST_MODULES = ("scripts/test_post_merge_fixtures.py",)


def test_modules() -> list[str]:
    """Every `test_*.py` file of `scripts/` and `contracts/`, as a path from the root."""
    return sorted(
        path.relative_to(ROOT).as_posix()
        for directory in ("scripts", "contracts")
        for path in (ROOT / directory).glob("test_*.py")
    )


def modules_a_step_runs() -> set[str]:
    """The test modules a step of check_all.sh runs, by a discover pattern or by name."""
    commands = STEP_COMMAND.findall((ROOT / "scripts/check_all.sh").read_text())
    runs = set()
    for path in test_modules():
        directory = posixpath.dirname(path)
        name = posixpath.basename(path)
        named = re.compile(rf"(?<![\w.-]){re.escape(name)}(?![\w.-])")
        for command in commands:
            discovered = any(
                posixpath.normpath(seen) == directory and fnmatch.fnmatchcase(name, pattern)
                for seen, pattern in DISCOVER_PATTERN.findall(command)
            )
            if discovered or named.search(command):
                runs.add(path)
                break
    return runs


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

    def test_the_fast_tier_runs_unit_tests_of_the_engine_and_the_whole_desktop_test_run(self):
        commands = (ROOT / "scripts/check_all.sh").read_text()
        engine = re.search(r"step engine fast engine:test-changed .*", commands)[0]
        self.assertIn("--lib --bins", engine)
        # The desktop's `tests/` directories run before the merge: no stand-in without them.
        desktop = re.search(r"step desktop both desktop:test .*", commands)[0]
        self.assertIn("cargo test --locked --workspace --all-features", desktop)
        self.assertNotIn("--lib", desktop)
        self.assertNotIn("desktop:test-fast", commands)
        self.assertIn("swift build --package-path apps/shared", commands)
        self.assertNotRegex(commands, r"step \w+ (?:both|fast) \S+ .*swift test")

    def test_the_end_to_end_set_runs_in_both_tiers(self):
        steps = {step_id: (area, tiers) for area, tiers, step_id in gate_steps()}
        for step_id in E2E_SET:
            with self.subTest(step=step_id):
                self.assertEqual(steps[step_id], ("e2e", "both"))
        # The real-agent suites need SOYEHT_REAL_ACP=1 and a real harness: after the merge only.
        for step_id, (area, tiers) in steps.items():
            if step_id.startswith("e2e:real-"):
                self.assertEqual((area, tiers), ("e2e", "full"))

    def test_the_end_to_end_programs_are_built_after_every_cargo_test_of_the_engine(self):
        # `cargo test` links target/debug/soyeht-engine from the library with the test hooks on.
        for tier in ("full", "fast"):
            order = listed_steps(tier, ENGINE_CORE)
            with self.subTest(tier=tier):
                tests = engine_cargo_tests()
                self.assertGreaterEqual(len(tests), 5, tests)
                last_test = max(order.index(i) for i in tests if i in order)
                self.assertGreater(order.index("engine:build-product"), last_test)
                self.assertGreater(order.index("engine:build-test-engine"), last_test)
                self.assertEqual([i for i in order if i in E2E_SET], list(E2E_SET))
        fast = listed_steps("fast", ENGINE_CORE)
        self.assertLess(fast.index("engine:test-changed"), fast.index("engine:build-product"))

    def test_the_swift_tests_build_every_engine_program_they_start(self):
        # The e2e steps can run on another host, so the Swift steps build what their tests start.
        swift_tests = "".join(
            path.read_text() for path in (ROOT / "apps/shared/Tests").rglob("*ConnectionTests.swift"))
        self.assertIn('"soyeht-engine-test"', swift_tests)
        text = (ROOT / "scripts/check_all.sh").read_text()
        self.assertRegex(text, r"(?m)^step swift full swift:engine-binaries build_engine_programs$")
        self.assertRegex(
            text, r"(?m)^build_engine_programs\(\) {\n    build_product_programs && build_test_engine\n}$")
        self.assertRegex(text, r"(?m)^build_test_engine\(\) {\n.*--bin soyeht-engine-test\n}$")

    def test_the_end_to_end_set_runs_on_the_remote_host_in_file_order(self):
        # remote_gate.sh runs the remote steps of one `check_all.sh --only`, which keeps file order,
        # so the builds follow the engine tests there only while both sit in a remote area.
        steps = {step_id: area for area, _, step_id in gate_steps()}
        for step_id in (*E2E_SET, "engine:test"):
            with self.subTest(step=step_id):
                self.assertIn(steps[step_id], post_merge_hosts.REMOTE_AREAS)

    def test_the_gate_script_unit_tests_have_their_own_area(self):
        steps = {step_id: (area, tiers) for area, tiers, step_id in gate_steps()}
        for step_id in GATE_SCRIPT_UNITS:
            with self.subTest(step=step_id):
                self.assertEqual(steps[step_id], ("gate_scripts", "both"))
        # The plan's own tests read the engine and desktop sources (CROSS_READS): every change runs them.
        self.assertEqual(steps["gate-plan:unit"], ("always", "both"))

    def test_every_test_module_of_the_scripts_and_contracts_runs_in_a_step(self):
        # A test module no step runs keeps passing until someone changes it, and nothing notices.
        runs = modules_a_step_runs()
        unrun = [path for path in test_modules()
                 if path not in runs and path not in HELPER_TEST_MODULES]
        self.assertEqual(unrun, [], "no step of scripts/check_all.sh runs these test modules")
        # The fixtures are imported by the tests above, so they are no test of their own.
        for path in HELPER_TEST_MODULES:
            with self.subTest(helper=path):
                self.assertTrue((ROOT / path).is_file())
                usage = re.compile(rf"\b(?:from|import)\s+{re.escape(Path(path).stem)}\b")
                imported = [other for other in test_modules()
                            if other != path and usage.search((ROOT / other).read_text())]
                self.assertNotEqual(imported, [], f"{path}: no test module imports it")

    def test_post_merge_prerequisites_are_earlier_steps_that_run_on_the_same_host(self):
        order = [step_id for _, _, step_id in gate_steps()]
        areas = {step_id: area for area, _, step_id in gate_steps()}
        for step_id, prerequisites in post_merge_gate.PREREQUISITES.items():
            self.assertIn(step_id, areas)
            for prerequisite in prerequisites:
                with self.subTest(step=step_id, prerequisite=prerequisite):
                    self.assertIn(prerequisite, areas)
                    self.assertLess(order.index(prerequisite), order.index(step_id))
                    # A selection is split between the Linux host and this machine by area.
                    self.assertEqual(areas[prerequisite] in post_merge_hosts.REMOTE_AREAS,
                                     areas[step_id] in post_merge_hosts.REMOTE_AREAS)

    def test_every_step_that_starts_the_engine_programs_takes_their_build_as_a_prerequisite(self):
        # A rerun of the step on a commit (the culprit search) must not run the programs of another
        # commit: the worktree keeps its target directory from one probe to the next.
        text = (ROOT / "scripts/check_all.sh").read_text()
        starts_programs = re.compile(r"scripts/e2e(?:_\w+)?\.py|swift test --package-path apps/shared")
        found = []
        for area, step_id, command in re.findall(r"^\s*step (\w+) \S+ (\S+) (.*)$", text, re.MULTILINE):
            if starts_programs.search(command) and "unittest" not in command:
                found.append(step_id)
                with self.subTest(step=step_id):
                    self.assertTrue(post_merge_gate.PREREQUISITES.get(step_id),
                                    f"{step_id} starts the engine's programs: build them first in a rerun")
        self.assertIn("e2e:core", found)
        self.assertIn("swift:test-shared", found)

    def test_the_attack_tests_take_the_engine_build_as_a_prerequisite(self):
        # They start the supervisor and the fake programs that only the workspace build leaves beside them.
        self.assertEqual(post_merge_gate.PREREQUISITES["security:attacks"], ("engine:build",))
        self.assertEqual(post_merge_gate.REMOTE_PREREQUISITES["security:attacks"], "engine:build")

    def test_the_fast_swift_client_test_builds_the_programs_it_starts_itself(self):
        # It sits in the `swift` area, so the e2e builds may have run on another host.
        text = (ROOT / "scripts/check_all.sh").read_text()
        self.assertRegex(text, r"(?m)^step swift fast swift:test-shared-fast swift_client_fast$")
        self.assertRegex(
            text, r"(?m)^swift_client_fast\(\) {\n    build_product_programs && uv run python scripts/e2e_swift_client\.py\n}$")
        self.assertIn(r"nativeClientReconnectsAndDrivesItsOwnShellAndEditor",
                      (ROOT / "scripts/e2e_swift_client.py").read_text())
        self.assertIn("nativeClientReconnectsAndDrivesItsOwnShellAndEditor",
                      (ROOT / "apps/shared/Tests/SoyehtClientTests/EngineConnectionTests.swift").read_text())

    def test_an_engine_change_runs_the_swift_client_test_between_the_tcc_snapshot_and_its_check(self):
        for files in (ENGINE_CORE, EDITOR_CRATE):
            order = listed_steps("fast", files)
            with self.subTest(files=files):
                self.assertLess(order.index("engine:build-product"), order.index("swift:test-shared-fast"))
                self.assertLess(order.index("e2e:tcc-snapshot"), order.index("swift:test-shared-fast"))
                self.assertLess(order.index("swift:test-shared-fast"), order.index("e2e:tcc-check"))
        for files in (DESKTOP_ONLY, DOCS_ONLY):
            with self.subTest(files=files):
                self.assertNotIn("swift:test-shared-fast", listed_steps("fast", files))
        self.assertNotIn("swift:test-shared-fast", listed_steps("full", ENGINE_CORE))

    def test_every_area_of_the_plan_has_a_step(self):
        used = {area for area, _, _ in gate_steps()}
        self.assertEqual(used, set(gate_plan.ALL_AREAS))

    def test_the_script_lists_every_step_of_each_tier(self):
        sample = ["engine/soyeht-web/src/lib.rs", "apps/desktop/crates/soyeht-client/src/lib.rs",
                  "apps/shared/Sources/A.swift"]
        for tier in ("fast", "full"):
            with self.subTest(tier=tier):
                listed = listed_steps(tier, sample)
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
                    self.assertIn("desktop:test", listed)
                    for step_id in AFTER_MERGE:
                        self.assertNotIn(step_id, listed)
                    self.assertIn("swift:test-shared-build", listed)


class SelectionByChangeTests(unittest.TestCase):
    """What `check_all.sh --tier fast` lists for each kind of change (gate_plan.py decides the areas)."""

    def test_a_desktop_only_change_runs_the_whole_desktop_test_run_and_no_e2e(self):
        listed = listed_steps("fast", DESKTOP_ONLY)
        self.assertIn("desktop:test", listed)
        self.assertNotIn("desktop:test-fast", listed)
        for step_id in (*E2E_SET, *GATE_SCRIPT_UNITS, "engine:test-changed"):
            self.assertNotIn(step_id, listed)

    def test_an_engine_core_change_runs_the_e2e_set_after_the_unit_tests(self):
        listed = listed_steps("fast", ENGINE_CORE)
        self.assertEqual([i for i in listed if i in E2E_SET], list(E2E_SET))
        self.assertIn("engine:test-changed", listed)
        self.assertNotIn("engine:test", listed)
        for step_id in GATE_SCRIPT_UNITS:
            self.assertNotIn(step_id, listed)

    def test_a_change_to_a_library_crate_the_engine_uses_runs_e2e_too(self):
        # gate_plan.py selects e2e when the reverse closure of the changed crate holds the engine,
        # the CLI, the supervisor or the MCP server; every engine library is under one of them.
        listed = listed_steps("fast", EDITOR_CRATE)
        self.assertEqual([i for i in listed if i in E2E_SET], list(E2E_SET))
        self.assertNotIn("desktop:test", listed)

    def test_an_android_change_lists_the_core_steps_and_not_the_app_build(self):
        # The core is built on the host in the fast tier; Gradle, the NDK and the APK only after the merge.
        listed = listed_steps("fast", ANDROID_APP)
        for step_id in ANDROID_CORE_STEPS:
            self.assertIn(step_id, listed)
        for step_id in ANDROID_APP_STEPS:
            self.assertNotIn(step_id, listed)
        full = listed_steps("full", ANDROID_APP)
        for step_id in (*ANDROID_CORE_STEPS, *ANDROID_APP_STEPS):
            self.assertIn(step_id, full)

    def test_what_the_android_core_reuses_lists_its_steps_and_a_docs_change_does_not(self):
        for files in (DESKTOP_ONLY, ["engine/soyeht-protocol/src/lib.rs"]):
            for step_id in ANDROID_CORE_STEPS:
                self.assertIn(step_id, listed_steps("fast", files), files)
        for step_id in (*ANDROID_CORE_STEPS, *ANDROID_APP_STEPS):
            self.assertNotIn(step_id, listed_steps("fast", DOCS_ONLY))

    def test_a_docs_only_change_runs_neither_e2e_nor_the_gate_script_unit_tests(self):
        listed = listed_steps("fast", DOCS_ONLY)
        for step_id in (*E2E_SET, *GATE_SCRIPT_UNITS, "desktop:test", "engine:test-changed"):
            self.assertNotIn(step_id, listed)
        self.assertIn("gate-plan:unit", listed)
        self.assertIn("doc-names:check", listed)

    def test_a_scripts_change_runs_the_gate_script_unit_tests(self):
        listed = listed_steps("fast", SCRIPTS_ONLY)
        for step_id in GATE_SCRIPT_UNITS:
            self.assertIn(step_id, listed)
        self.assertIn("gate-plan:unit", listed)

    def test_the_full_tier_lists_all_of_them(self):
        listed = listed_steps("full", DOCS_ONLY)
        for step_id in (*E2E_SET, *GATE_SCRIPT_UNITS, "desktop:test", "engine:test"):
            self.assertIn(step_id, listed)


class VocabularySkipTests(unittest.TestCase):
    """Runs the script itself: a variable that is read but never set only fails when it runs."""

    def run_script(self, *arguments: str, vocabulary_file: str = "", **environment: str):
        scratch = Path(tempfile.mkdtemp())
        timings = scratch / "timings.json"
        # The vocabulary file is named here, so a developer's own list never reaches the test.
        listed = scratch / "vocabulary.txt"
        listed.write_text(vocabulary_file)
        env = {key: value for key, value in os.environ.items()
               if key not in ("FORBIDDEN_TERMS", "SOYEHT_SKIP_FORBIDDEN_TERMS")}
        env.update(environment, SOYEHT_GATE_TIMINGS=str(timings), GIT_CONFIG_COUNT="2",
                   GIT_CONFIG_KEY_0="soyeht.vocabularyFile", GIT_CONFIG_VALUE_0=str(listed),
                   GIT_CONFIG_KEY_1="soyeht.requireVocabulary", GIT_CONFIG_VALUE_1="false")
        result = subprocess.run(
            ["bash", "-u", "scripts/check_all.sh", *arguments],
            cwd=ROOT, capture_output=True, text=True, env=env,
        )
        return result, timings

    def test_listing_works_with_and_without_the_vocabulary_or_the_flag(self):
        for environment in ({}, {"FORBIDDEN_TERMS": "dummy"}, {"SOYEHT_SKIP_FORBIDDEN_TERMS": "1"},
                            {"FORBIDDEN_TERMS": "dummy", "SOYEHT_SKIP_FORBIDDEN_TERMS": "1"}):
            with self.subTest(environment=environment):
                result, _ = self.run_script("--list", "--tier", "fast", **environment)
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_a_run_without_the_vocabulary_or_the_flag_is_refused(self):
        result, _ = self.run_script("--only", "gate-timings:unit")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SOYEHT_SKIP_FORBIDDEN_TERMS", result.stderr)

    def test_a_skipped_run_passes_but_says_and_records_that_it_is_not_merge_grade(self):
        result, timings = self.run_script("--only", "gate-timings:unit", SOYEHT_SKIP_FORBIDDEN_TERMS="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("vocabulary scan SKIPPED, not merge-grade", result.stdout)
        self.assertIn("vocabulary-skipped", timings.read_text())

    def test_supplied_terms_or_a_vocabulary_file_make_a_normal_run_even_with_the_flag(self):
        for supplied in ({"FORBIDDEN_TERMS": "dummy"}, {"vocabulary_file": "sampleword\n"}):
            with self.subTest(supplied=list(supplied)):
                result, timings = self.run_script(
                    "--only", "gate-timings:unit", SOYEHT_SKIP_FORBIDDEN_TERMS="1", **supplied)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertNotIn("SKIPPED", result.stdout + result.stderr)
                self.assertNotIn("vocabulary-skipped", timings.read_text())
                self.assertNotIn("sampleword", result.stdout + result.stderr)

    def test_the_remote_gate_exports_no_stand_in_list(self):
        text = (ROOT / "scripts" / "remote_gate.sh").read_text()
        self.assertNotIn("export FORBIDDEN_TERMS", text)
        self.assertIn("export SOYEHT_SKIP_FORBIDDEN_TERMS=1", text)


SUITE_RUN = re.compile(r"\b(?:swift|cargo)\s+test\b|\bunittest\b|\bnpm\s+(?:run\s+)?test\b")
BUILD_STEP_SCRIPT = re.compile(
    r"^\s*step\s+\S+\s+\S+\s+\S+:build\s+bash\s+(scripts/\S+\.sh)\s*$", re.MULTILINE
)


def suite_runs(script: str) -> list[str]:
    """The lines of a shell script, comments left out, that start a test suite."""
    code = (line for line in script.splitlines() if not line.lstrip().startswith("#"))
    return [line.strip() for line in code if SUITE_RUN.search(line)]


class BuildStepTests(unittest.TestCase):
    """A step that builds an app fails only when the build fails.

    The Mac app step once ran the shared package's whole suite before `xcodebuild`, a second run
    of `swift:test-shared`. On a loaded host a timing-sensitive test failed in it, and the watch
    counted a flake of `mac-app:build` although the build never ran (three of its failures in a
    row were test failures). A test suite is a step of its own, listed once.
    """

    def test_the_build_scripts_are_found(self):
        scripts = BUILD_STEP_SCRIPT.findall((ROOT / "scripts/check_all.sh").read_text())
        self.assertIn("scripts/check_mac.sh", scripts)
        self.assertIn("scripts/check_ios.sh", scripts)

    def test_a_build_step_runs_no_test_suite(self):
        scripts = BUILD_STEP_SCRIPT.findall((ROOT / "scripts/check_all.sh").read_text())
        for script in scripts:
            self.assertEqual(
                suite_runs((ROOT / script).read_text()), [],
                f"{script} runs a test suite; make it a step of check_all.sh",
            )

    def test_a_script_that_runs_a_suite_is_recognised(self):
        script = "# swift test is not run here\nswift test --package-path apps/shared\nxcodebuild build\n"
        self.assertEqual(suite_runs(script), ["swift test --package-path apps/shared"])


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
