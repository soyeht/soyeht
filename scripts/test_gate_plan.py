import contextlib
import io
import json
import os
import re
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import gate_base
import gate_plan
from gate_plan import Package, make_plan

PACKAGES = [
    Package("soyeht-protocol", "engine/soyeht-protocol", ()),
    Package("soyeht-platform", "engine/soyeht-platform", ()),
    Package("soyeht-process", "engine/soyeht-process", ("soyeht-platform",)),
    Package("soyeht-vcs", "engine/soyeht-vcs", ("soyeht-process", "soyeht-platform")),
    Package("soyeht-editor", "engine/soyeht-editor", ()),
    Package("soyeht-web", "engine/soyeht-web", ()),
    Package("soyeht-store", "engine/soyeht-store", ()),
    Package("soyeht-ptyd", "engine/soyeht-ptyd", ("soyeht-platform",)),
    Package("soyeht-mcp", "engine/soyeht-mcp", ("soyeht-protocol",)),
    Package("soyeht-engine", "engine/soyeht-engine",
            ("soyeht-protocol", "soyeht-vcs", "soyeht-editor", "soyeht-process", "soyeht-web",
             "soyeht-store", "soyeht-ptyd")),
    Package("soyeht-cli", "engine/soyeht-cli", ("soyeht-protocol", "soyeht-platform")),
]
DESKTOP_DEPS = {"soyeht-protocol", "soyeht-platform"}
EVERYTHING = ("always", "engine", "desktop", "swift", "typescript", "e2e")
# A change under scripts/ also selects the unit tests of the gate scripts.
EVERYTHING_AND_SCRIPTS = ("always", "gate_scripts", "engine", "desktop", "swift", "typescript", "e2e")


def plan(*files: str):
    return make_plan(list(files), PACKAGES, DESKTOP_DEPS)


class AreaTests(unittest.TestCase):
    def test_docs_only_selects_always(self):
        result = plan("docs/adr/0089-fast-gate.md", "AGENTS.md", "apps/desktop/README.md",
                      "design/soyeht-desktop.pen")
        self.assertEqual(result.areas, ("always",))
        self.assertEqual(result.review, "optional")
        for area in ("desktop", "e2e", "gate_scripts"):
            self.assertNotIn(area, result.areas)

    def test_markdown_outside_the_document_directories_is_not_a_document(self):
        self.assertEqual(plan("tools/NOTES.md").areas, EVERYTHING)
        self.assertIn("soyeht-web", plan("engine/soyeht-web/NOTES.md").engine_crates)

    def test_ratchet_data_is_not_gate_tooling(self):
        for path in ("scripts/quality_baseline.json", "scripts/quality_baseline_justifications.json",
                     "scripts/dead_pub_allowlist.json", "scripts/dead_public_swift_allowlist.json",
                     "scripts/other_allowlist.json"):
            with self.subTest(path=path):
                self.assertEqual(plan(path).areas, ("always", "gate_scripts"))

    def test_empty_change_selects_always(self):
        self.assertEqual(plan().areas, ("always",))

    def test_an_engine_crate_selects_it_and_its_reverse_dependencies(self):
        result = plan("engine/soyeht-vcs/src/lib.rs")
        self.assertEqual(result.areas, ("always", "engine", "e2e"))
        self.assertEqual(result.engine_crates, ("soyeht-engine", "soyeht-vcs"))

    def test_unit_tests_do_not_follow_the_edges_that_only_integration_tests_use(self):
        # A sibling-binary test (the CLI starts the engine) runs after the merge, not before it.
        self.assertEqual(plan("engine/soyeht-mcp/src/main.rs").engine_crates, ("soyeht-mcp",))
        self.assertNotIn("soyeht-cli", plan("engine/soyeht-ptyd/src/lib.rs").engine_crates)
        self.assertNotIn("soyeht-platform", plan("engine/soyeht-editor/src/lib.rs").engine_crates)

    def test_an_engine_change_leaves_the_swift_tests_to_the_full_tier(self):
        for path in ("engine/soyeht-engine/src/x.rs", "engine/soyeht-ptyd/src/x.rs",
                     "engine/soyeht-cli/src/x.rs", "engine/soyeht-store/src/x.rs"):
            with self.subTest(path=path):
                self.assertNotIn("swift", plan(path).areas)

    def test_a_crate_the_desktop_uses_selects_desktop_too(self):
        self.assertEqual(plan("engine/soyeht-platform/src/fs.rs").areas,
                         ("always", "engine", "desktop", "e2e"))
        self.assertIsNotNone(plan("engine/soyeht-platform/src/fs.rs").engine_crates)

    def test_an_engine_file_outside_every_crate_tests_the_workspace(self):
        result = plan("engine/rustfmt.toml")
        self.assertEqual(result.areas, EVERYTHING)
        self.assertIsNone(result.engine_crates)

    def test_unreadable_workspace_selects_every_code_area(self):
        result = make_plan(["engine/soyeht-vcs/src/lib.rs"], None, set())
        self.assertEqual(result.areas, EVERYTHING)
        self.assertIsNone(result.engine_crates)
        self.assertEqual(make_plan(["docs/a.md"], None, set()).areas, EVERYTHING)

    def test_desktop_only(self):
        result = plan("apps/desktop/crates/soyeht-desktop/src/app.rs")
        self.assertEqual(result.areas, ("always", "desktop"))

    def test_swift_and_dictation(self):
        # The app builds and the dictation tests have no fast step; the full tier runs them.
        for path in ("apps/mac/Sources/Foo.swift", "apps/ios/Sources/Foo.swift",
                     "apps/shared/Sources/A.swift", "contracts/swift/Package.swift",
                     "apps/dictation/Sources/A.swift"):
            with self.subTest(path=path):
                self.assertEqual(plan(path).areas, ("always", "swift"))

    def test_typescript(self):
        self.assertEqual(plan("contracts/typescript/src/x.ts").areas, ("always", "typescript"))

    def test_cross_cutting_paths_select_every_code_area(self):
        for path in (
            "contracts/protocol.json",
            "contracts/profiles.json",
            "contracts/design-tokens.json",
            "contracts/generate.py",
            "contracts/vectors/messages.json",
            "engine/soyeht-protocol/src/lib.rs",
            "scripts/check_all.sh",
            "scripts/gate_plan.py",
            "scripts/e2e.py",
            ".github/workflows/check-all.yml",
            "engine/Cargo.lock",
            "apps/desktop/Cargo.toml",
            "rust-toolchain.toml",
        ):
            with self.subTest(path=path):
                result = plan(path)
                scripts = path.startswith("scripts/") or path == "contracts/generate.py"
                self.assertEqual(result.areas, EVERYTHING_AND_SCRIPTS if scripts else EVERYTHING)
                self.assertIsNone(result.engine_crates)

    def test_unknown_paths_select_everything(self):
        self.assertEqual(plan("tools/new/thing.bin").areas, EVERYTHING)

    def test_prompts_compiled_into_the_engine_are_not_documentation(self):
        result = plan("docs/contracts/prompts/harness-switch.en.md")
        self.assertEqual(result.areas, EVERYTHING)

    def test_areas_combine(self):
        result = plan("docs/x.md", "apps/desktop/crates/a/src/b.rs", "engine/soyeht-editor/src/lib.rs")
        self.assertEqual(result.areas, ("always", "engine", "desktop", "e2e"))
        self.assertIn("soyeht-editor", result.engine_crates)

    def test_full_plan_has_every_area(self):
        self.assertEqual(gate_plan.full_plan().areas, gate_plan.ALL_AREAS)

    def test_files_that_other_directories_read(self):
        project = plan("apps/mac/SoyehtMac.xcodeproj/project.pbxproj")
        self.assertEqual(project.areas, ("always", "engine", "swift", "e2e"))
        self.assertIn("soyeht-engine", project.engine_crates)
        fonts = plan("apps/mac/Resources/Fonts/Inter.ttf")
        self.assertEqual(fonts.areas, ("always", "desktop", "swift"))
        icons = plan("apps/shared/Sources/SoyehtClient/Resources/ProviderIcons.xcassets/a.png")
        self.assertIn("desktop", icons.areas)
        self.assertIn("swift", icons.areas)

    def test_gate_scripts_upgrade_the_run_to_the_full_tier(self):
        for path in ("scripts/check_all.sh", "scripts/gate_plan.py", "scripts/post_merge.py",
                     "scripts/e2e_agents.py", "scripts/check_mac.sh", ".github/workflows/check-all.yml"):
            with self.subTest(path=path):
                self.assertEqual(plan(path).upgrade, (path,))
        for path in ("scripts/check_quality.py", "docs/a.md", "engine/soyeht-web/src/lib.rs"):
            with self.subTest(path=path):
                self.assertEqual(plan(path).upgrade, ())
        self.assertIn("GATE_UPGRADE=scripts/check_all.sh", gate_plan.render_shell(plan("scripts/check_all.sh")))

    def test_the_new_gate_scripts_upgrade_the_run_too(self):
        for path in ("scripts/main_watch.py", "scripts/remote_gate.sh", "scripts/post_merge_hosts.py",
                     "scripts/post_merge_gate.py", "scripts/post_merge_investigate.py",
                     "scripts/post_merge_culprit.py", "scripts/post_merge_editor.py",
                     "scripts/post_merge_records.py"):
            with self.subTest(path=path):
                self.assertEqual(plan(path).upgrade, (path,))
                self.assertEqual(plan(path).areas, EVERYTHING_AND_SCRIPTS)

    def test_a_test_only_change_of_the_gate_scripts_stays_in_the_fast_tier(self):
        for path in ("scripts/test_gate_plan.py", "scripts/test_post_merge.py", "scripts/test_main_watch.py",
                     "scripts/test_gate_list.py", "scripts/test_tcc_guard.py", "scripts/test_check_quality.py",
                     "scripts/test_post_merge_fixtures.py", "scripts/test_post_merge_load.py"):
            with self.subTest(path=path):
                result = plan(path)
                self.assertEqual(result.upgrade, ())
                self.assertEqual(result.areas, ("always", "gate_scripts"))
                self.assertEqual(result.review, "required")  # a weaker test is a reviewer's call
        # Beside the script it tests, the script decides.
        both = plan("scripts/test_post_merge.py", "scripts/post_merge.py")
        self.assertEqual(both.upgrade, ("scripts/post_merge.py",))
        self.assertEqual(both.areas, EVERYTHING_AND_SCRIPTS)

    def test_only_unit_tests_of_the_scripts_directory_are_test_only(self):
        self.assertFalse(gate_plan.gate_test("scripts/not_a_test.py"))
        self.assertFalse(gate_plan.gate_test("scripts/test_data.json"))
        self.assertFalse(gate_plan.gate_test("scripts/quality/test_x.py"))
        self.assertEqual(plan("scripts/quality/test_x.py").areas, EVERYTHING_AND_SCRIPTS)


class DesktopTestSelectionTests(unittest.TestCase):
    """The desktop runs its whole `cargo test` (check_all.sh `desktop:test`) whenever it is selected."""

    def test_a_desktop_only_change_selects_the_desktop_and_nothing_that_needs_the_engine(self):
        for path in ("apps/desktop/crates/soyeht-client/tests/engine.rs",
                     "apps/desktop/crates/soyeht-desktop/src/app.rs"):
            with self.subTest(path=path):
                result = plan(path)
                self.assertEqual(result.areas, ("always", "desktop"))
                self.assertNotIn("e2e", result.areas)
                self.assertNotIn("gate_scripts", result.areas)

    def test_an_engine_crate_the_desktop_depends_on_selects_the_desktop_tests_too(self):
        self.assertIn("desktop", plan("engine/soyeht-protocol/src/lib.rs").areas)
        self.assertIn("desktop", plan("engine/soyeht-platform/src/fs.rs").areas)


class EndToEndSelectionTests(unittest.TestCase):
    """e2e runs when the engine selection is the workspace or reaches a program the suites start."""

    def test_an_engine_core_change_selects_e2e(self):
        for path in ("engine/soyeht-engine/src/core.rs", "engine/soyeht-engine/src/usage/probe.rs",
                     "engine/soyeht-store/src/lib.rs"):
            with self.subTest(path=path):
                result = plan(path)
                self.assertIn("e2e", result.areas)
                self.assertIn("soyeht-engine", result.engine_crates)

    def test_each_program_the_suites_start_selects_e2e(self):
        for path in ("engine/soyeht-ptyd/src/lib.rs", "engine/soyeht-cli/src/main.rs",
                     "engine/soyeht-mcp/src/main.rs", "engine/soyeht-engine/src/main.rs"):
            with self.subTest(path=path):
                self.assertIn("e2e", plan(path).areas)

    def test_a_library_crate_selects_e2e_when_a_program_depends_on_it(self):
        # soyeht-editor is a dependency of soyeht-engine, so its reverse closure holds the engine.
        result = plan("engine/soyeht-editor/src/lib.rs")
        self.assertEqual(result.engine_crates, ("soyeht-editor", "soyeht-engine"))
        self.assertEqual(result.areas, ("always", "engine", "e2e"))

    def test_a_library_crate_that_no_program_depends_on_does_not_select_e2e(self):
        leaf = [*PACKAGES, Package("soyeht-leaf", "engine/soyeht-leaf", ("soyeht-editor",))]
        result = make_plan(["engine/soyeht-leaf/src/lib.rs"], leaf, DESKTOP_DEPS)
        self.assertEqual(result.engine_crates, ("soyeht-leaf",))
        self.assertEqual(result.areas, ("always", "engine"))

    def test_the_whole_workspace_selects_e2e(self):
        for path in ("engine/rustfmt.toml", "engine/Cargo.lock", "contracts/protocol.json"):
            with self.subTest(path=path):
                result = plan(path)
                self.assertIsNone(result.engine_crates)
                self.assertIn("e2e", result.areas)
        self.assertIn("e2e", make_plan(["engine/soyeht-vcs/src/lib.rs"], None, set()).areas)

    def test_prompts_and_project_files_that_the_engine_reads_select_e2e(self):
        self.assertIn("e2e", plan("docs/contracts/prompts/harness-switch.en.md").areas)
        self.assertIn("e2e", plan("apps/mac/SoyehtMac.xcodeproj/project.pbxproj").areas)

    def test_changes_that_do_not_reach_the_engine_do_not_select_e2e(self):
        for path in ("docs/adr/0089-fast-gate.md", "AGENTS.md", "apps/desktop/crates/a/src/b.rs",
                     "apps/shared/Sources/A.swift", "apps/mac/Sources/Foo.swift",
                     "contracts/typescript/src/x.ts", "scripts/test_gate_plan.py",
                     "scripts/quality_baseline.json"):
            with self.subTest(path=path):
                self.assertNotIn("e2e", plan(path).areas)


class GateScriptsSelectionTests(unittest.TestCase):
    """The unit tests of the gate scripts run when something that could break them changed."""

    def test_a_change_to_the_scripts_selects_their_tests(self):
        for path in ("scripts/check_quality.py", "scripts/test_check_quality.py", "scripts/post_merge.py",
                     "scripts/check_all.sh", "scripts/quality_baseline.json", "scripts/quality/x.js",
                     "contracts/generate.py", "contracts/test_generator.py", ".githooks/pre-push"):
            with self.subTest(path=path):
                self.assertIn("gate_scripts", plan(path).areas)

    def test_other_changes_do_not(self):
        for path in ("docs/adr/0089-fast-gate.md", "AGENTS.md", "engine/soyeht-engine/src/core.rs",
                     "apps/desktop/crates/a/src/b.rs", "apps/shared/Sources/A.swift",
                     "contracts/protocol.json", "contracts/vectors/messages.json",
                     "contracts/typescript/src/x.ts", "contracts/swift/Package.swift"):
            with self.subTest(path=path):
                self.assertNotIn("gate_scripts", plan(path).areas)

    def test_the_full_tier_has_them(self):
        self.assertIn("gate_scripts", gate_plan.full_plan().areas)
        self.assertIn("e2e", gate_plan.full_plan().areas)

    def test_the_unit_tests_of_the_plan_stay_in_the_always_area(self):
        # test_gate_plan reads engine and desktop sources (CROSS_READS), so any change can break it.
        text = (gate_plan.ROOT / "scripts/check_all.sh").read_text()
        self.assertRegex(text, r"(?m)^step always both gate-plan:unit ")


class ReviewTests(unittest.TestCase):
    def test_required_paths(self):
        for path in (
            "engine/soyeht-engine/src/core.rs",
            "engine/soyeht-engine/src/core_lock.rs",
            "engine/soyeht-engine/src/dispatch.rs",
            "engine/soyeht-engine/src/server.rs",
            "engine/soyeht-engine/src/agents/mcp.rs",
            "engine/soyeht-store/src/lib.rs",
            "engine/soyeht-protocol/src/lib.rs",
            "contracts/protocol.json",
            "contracts/profiles.json",
            "engine/soyeht-engine/src/ui_attestation.rs",
            "engine/soyeht-web/src/permissions.rs",
            "engine/soyeht-ptyd/src/lib.rs",
            "engine/soyeht-platform/src/anything.rs",
            "engine/soyeht-web/Cargo.toml",
            "Cargo.lock",
            "scripts/quality_baseline_justifications.json",
            "scripts/dead_pub_allowlist.json",
            "scripts/dead_public_swift_allowlist.json",
            "apps/mac/Sources/EditorView.swift",
            "apps/desktop/crates/soyeht-desktop/src/editor_view.rs",
            "apps/desktop/crates/soyeht-client/src/editor/state.rs",
            "engine/soyeht-editor/src/lib.rs",
            "engine/soyeht-engine/src/editor/mod.rs",
            "engine/soyeht-platform/src/fs.rs",
            "engine/soyeht-engine/src/auth.rs",
            "engine/soyeht-engine/src/push/mod.rs",
            "scripts/check_all.sh",
        ):
            with self.subTest(path=path):
                self.assertEqual(plan(path).review, "required")

    def test_optional_paths(self):
        for path in (
            "docs/adr/0001.md",
            "apps/desktop/crates/soyeht-desktop/src/app.rs",
            "engine/soyeht-engine/src/settings.rs",
            "engine/soyeht-web/src/lib.rs",
            "scripts/quality_baseline.json",
        ):
            with self.subTest(path=path):
                self.assertEqual(plan(path).review, "optional")


class RenderTests(unittest.TestCase):
    def test_shell_output_is_assignments_only(self):
        text = gate_plan.render_shell(plan("engine/soyeht-vcs/src/lib.rs"))
        self.assertIn("GATE_AREAS='always engine e2e'", text)
        self.assertIn("GATE_ENGINE_TEST_ARGS='-p soyeht-engine -p soyeht-vcs'", text)

    def test_shell_output_for_everything_tests_the_workspace(self):
        self.assertIn("GATE_ENGINE_TEST_ARGS=--workspace", gate_plan.render_shell(plan("scripts/x.py")))

    def test_text_names_the_deferred_areas(self):
        text = gate_plan.render_text(plan("docs/a.md"), "fast")
        self.assertIn("after the merge only: dictation mac_app ios_app editor_perf timing", text)
        self.assertNotIn("e2e", text.split("after the merge only:")[1])

    def test_cargo_metadata_is_turned_into_packages(self):
        raw = [
            {"name": "a", "manifest_path": str(gate_plan.ROOT / "engine/a/Cargo.toml"),
             "dependencies": [{"name": "b"}, {"name": "serde"}]},
            {"name": "b", "manifest_path": str(gate_plan.ROOT / "engine/b/Cargo.toml"),
             "dependencies": []},
        ]
        packages = gate_plan.to_packages(raw)
        self.assertEqual(packages[0], Package("a", "engine/a", ("b",)))

    def test_desktop_dependencies_in_the_engine(self):
        raw = [{"name": "client", "dependencies": [
            {"name": "soyeht-protocol", "path": str(gate_plan.ROOT / "engine/soyeht-protocol")},
            {"name": "local", "path": str(gate_plan.ROOT / "apps/desktop/crates/local")},
            {"name": "serde"},
        ]}]
        self.assertEqual(gate_plan.desktop_engine_dependencies(raw), {"soyeht-protocol"})


UP = re.compile(r"(?:\.\./)+[\w./-]*")


def crate_packages() -> list[Package]:
    packages = []
    for manifest in sorted(gate_plan.ROOT.glob("engine/*/Cargo.toml")):
        name = re.search(r'(?m)^name\s*=\s*"([^"]+)"', manifest.read_text())
        packages.append(Package(name.group(1), manifest.parent.relative_to(gate_plan.ROOT).as_posix(), ()))
    return packages


def reads_leaving_the_crate():
    """(source file, repository path) for each literal that climbs out of its crate."""
    roots = [(gate_plan.ROOT / "engine", 2), (gate_plan.ROOT / "apps/desktop/crates", 3)]
    for top, depth in roots:
        for source in sorted(top.rglob("*.rs")):
            relative = source.relative_to(gate_plan.ROOT)
            if "target" in relative.parts:
                continue
            crate = gate_plan.ROOT.joinpath(*relative.parts[:depth])
            text = source.read_text(errors="replace")
            for literal in re.findall(r'"([^"\n]*)"', text):
                for climb in UP.findall(literal):
                    # Cut a format placeholder: only the fixed prefix names a file.
                    fixed = climb.split("{")[0]
                    for origin in (source.parent, crate):
                        target = (origin / fixed).resolve()
                        inside = target == crate or crate in target.parents
                        if target.exists() and not inside and gate_plan.ROOT in target.parents:
                            yield relative.as_posix(), target.relative_to(gate_plan.ROOT).as_posix()


class CrossReadTests(unittest.TestCase):
    def test_every_read_that_leaves_a_crate_is_covered_by_the_plan(self):
        packages = crate_packages()
        desktop_deps = {"soyeht-protocol", "soyeht-platform"}
        seen = set(reads_leaving_the_crate())
        self.assertTrue(seen, "the scan found no cross-directory read; fix the scan")
        problems = []
        for source, target in sorted(seen):
            result = make_plan([target], packages, desktop_deps)
            if source.startswith("apps/desktop/"):
                covered = "desktop" in result.areas
            else:
                consumer = next(p.name for p in packages if source.startswith(p.directory + "/"))
                covered = "engine" in result.areas and (
                    result.engine_crates is None or consumer in result.engine_crates
                )
            if not covered:
                problems.append(f"{source} reads {target}, which does not select its tests; add it to CROSS_READS")
        self.assertEqual(problems, [])

    def test_the_known_reads_are_found(self):
        seen = set(reads_leaving_the_crate())
        self.assertIn(("engine/soyeht-engine/src/ui_attestation.rs", "apps/mac/SoyehtMac.xcodeproj/project.pbxproj"), seen)
        self.assertTrue(any(source.endswith("fonts.rs") and "mac/Resources/Fonts" in target for source, target in seen))


class MainHealthTests(unittest.TestCase):
    def health(self, record):
        directory = Path(tempfile.mkdtemp())
        if record is not None:
            (directory / "latest.json").write_text(json.dumps(record))
        return directory

    def test_a_red_main_is_a_warning(self):
        warning = gate_plan.main_health_warning(self.health(
            {"sha": "a" * 40, "result": "fail", "failed_step": "engine:test", "culprit": {"sha": "b" * 40}}))
        self.assertIn("main is RED", warning)
        self.assertIn("engine:test", warning)
        self.assertIn("b" * 12, warning)

    def test_a_flaky_head_is_a_note(self):
        note = gate_plan.main_health_warning(self.health(
            {"sha": "c" * 40, "result": "flaky", "failed_step": "swift:test-shared"}))
        self.assertIn("main is FLAKY at " + "c" * 12, note)
        self.assertIn("swift:test-shared", note)
        self.assertNotIn("RED", note)

    def test_the_directory_is_the_environment_s_and_the_plan_reads_what_the_operator_writes(self):
        written = Path(tempfile.mkdtemp())
        (written / "latest.json").write_text(json.dumps(
            {"sha": "d" * 40, "result": "fail", "failed_step": "engine:test"}))
        with mock.patch.dict(os.environ, {gate_base.MAIN_HEALTH_DIR_ENV: str(written)}):
            self.assertEqual(gate_base.main_health_dir(gate_plan.ROOT), written.resolve())
            self.assertIn("main is RED", gate_plan.main_health_warning(gate_base.main_health_dir(gate_plan.ROOT)))

    def test_the_default_directory_is_the_main_checkouts_cache(self):
        with mock.patch.dict(os.environ):
            os.environ.pop(gate_base.MAIN_HEALTH_DIR_ENV, None)
            self.assertEqual(gate_base.main_health_dir(gate_plan.ROOT),
                             gate_base.main_checkout(gate_plan.ROOT) / ".cache/main-health")

    def test_the_fast_plan_prints_the_warning_of_the_environment_directory(self):
        written = Path(tempfile.mkdtemp())
        (written / "latest.json").write_text(json.dumps(
            {"sha": "e" * 40, "result": "fail", "failed_step": "engine:test", "culprit": {"sha": "f" * 40}}))
        files = written / "files"
        files.write_text("README.md\n")
        error = io.StringIO()
        with mock.patch.dict(os.environ, {gate_base.MAIN_HEALTH_DIR_ENV: str(written)}), \
                contextlib.redirect_stderr(error), contextlib.redirect_stdout(io.StringIO()):
            gate_plan.main(["--tier", "fast", "--files-from", str(files)])
        self.assertIn("WARNING: main is RED at " + "e" * 12, error.getvalue())

    def test_green_or_unrecorded_main_is_silent(self):
        self.assertIsNone(gate_plan.main_health_warning(self.health({"sha": "a", "result": "pass"})))
        self.assertIsNone(gate_plan.main_health_warning(self.health(None)))


if __name__ == "__main__":
    unittest.main()
