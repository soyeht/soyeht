import json
import re
import tempfile
import unittest
from pathlib import Path

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
EVERYTHING = ("always", "engine", "desktop", "swift", "dictation", "mac_app", "ios_app", "typescript")


def plan(*files: str):
    return make_plan(list(files), PACKAGES, DESKTOP_DEPS)


class AreaTests(unittest.TestCase):
    def test_docs_only_selects_always(self):
        result = plan("docs/adr/0089-fast-gate.md", "AGENTS.md", "apps/desktop/README.md",
                      "design/soyeht-desktop.pen")
        self.assertEqual(result.areas, ("always",))
        self.assertEqual(result.review, "optional")

    def test_markdown_outside_the_document_directories_is_not_a_document(self):
        self.assertEqual(plan("tools/NOTES.md").areas, EVERYTHING)
        self.assertIn("soyeht-web", plan("engine/soyeht-web/NOTES.md").engine_crates)

    def test_ratchet_data_is_not_gate_tooling(self):
        for path in ("scripts/quality_baseline.json", "scripts/quality_baseline_justifications.json",
                     "scripts/dead_pub_allowlist.json", "scripts/other_allowlist.json"):
            with self.subTest(path=path):
                self.assertEqual(plan(path).areas, ("always",))

    def test_empty_change_selects_always(self):
        self.assertEqual(plan().areas, ("always",))

    def test_an_engine_crate_selects_it_and_its_reverse_dependencies(self):
        result = plan("engine/soyeht-vcs/src/lib.rs")
        self.assertEqual(result.areas, ("always", "engine", "swift"))
        self.assertEqual(
            result.engine_crates,
            ("soyeht-cli", "soyeht-engine", "soyeht-platform", "soyeht-vcs"),
        )

    def test_every_engine_change_also_tests_the_platform_and_the_cli(self):
        result = plan("engine/soyeht-editor/src/lib.rs")
        self.assertIn("soyeht-platform", result.engine_crates)
        self.assertIn("soyeht-cli", result.engine_crates)

    def test_test_time_edges_the_graph_does_not_show(self):
        self.assertIn("soyeht-engine", plan("engine/soyeht-mcp/src/main.rs").engine_crates)
        ptyd = plan("engine/soyeht-ptyd/src/lib.rs").engine_crates
        self.assertIn("soyeht-cli", ptyd)
        self.assertIn("soyeht-engine", ptyd)
        self.assertIn("soyeht-cli", plan("engine/soyeht-engine/src/auth.rs").engine_crates)

    def test_engine_crates_the_swift_tests_launch_also_run_the_shared_swift_tests(self):
        for path in ("engine/soyeht-engine/src/x.rs", "engine/soyeht-ptyd/src/x.rs",
                     "engine/soyeht-cli/src/x.rs", "engine/soyeht-store/src/x.rs",
                     "engine/soyeht-web/src/x.rs"):
            with self.subTest(path=path):
                self.assertIn("swift", plan(path).areas)
        self.assertNotIn("dictation", plan("engine/soyeht-store/src/x.rs").areas)

    def test_a_crate_the_desktop_uses_selects_desktop_too(self):
        self.assertEqual(plan("engine/soyeht-platform/src/fs.rs").areas,
                         ("always", "engine", "desktop", "swift"))
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
        self.assertEqual(plan("apps/mac/Sources/Foo.swift").areas, ("always", "swift", "mac_app"))
        self.assertEqual(plan("apps/ios/Sources/Foo.swift").areas, ("always", "swift", "ios_app"))
        shared = ("always", "swift", "dictation", "mac_app", "ios_app")
        self.assertEqual(plan("apps/shared/Sources/A.swift").areas, shared)
        self.assertEqual(plan("contracts/swift/Package.swift").areas, shared)
        self.assertEqual(plan("apps/dictation/Sources/A.swift").areas, ("always", "swift", "dictation"))

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
                self.assertEqual(result.areas, EVERYTHING)
                self.assertIsNone(result.engine_crates)
                self.assertTrue(result.dictation)

    def test_unknown_paths_select_everything(self):
        self.assertEqual(plan("tools/new/thing.bin").areas, EVERYTHING)

    def test_prompts_compiled_into_the_engine_are_not_documentation(self):
        result = plan("docs/contracts/prompts/harness-switch.en.md")
        self.assertEqual(result.areas, EVERYTHING)

    def test_areas_combine(self):
        result = plan("docs/x.md", "apps/desktop/crates/a/src/b.rs", "engine/soyeht-editor/src/lib.rs")
        self.assertEqual(result.areas, ("always", "engine", "desktop", "swift"))
        self.assertIn("soyeht-editor", result.engine_crates)

    def test_full_plan_has_every_area(self):
        self.assertEqual(gate_plan.full_plan().areas, gate_plan.ALL_AREAS)

    def test_files_that_other_directories_read(self):
        project = plan("apps/mac/SoyehtMac.xcodeproj/project.pbxproj")
        self.assertEqual(project.areas, ("always", "engine", "swift", "mac_app"))
        self.assertIn("soyeht-engine", project.engine_crates)
        fonts = plan("apps/mac/Resources/Fonts/Inter.ttf")
        self.assertEqual(fonts.areas, ("always", "desktop", "swift", "mac_app"))
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
        self.assertIn("GATE_AREAS='always engine swift'", text)
        self.assertIn("GATE_ENGINE_TEST_ARGS='-p soyeht-cli -p soyeht-engine -p soyeht-platform -p soyeht-vcs'", text)
        self.assertIn("GATE_DICTATION=0", text)

    def test_shell_output_for_everything_tests_the_workspace(self):
        self.assertIn("GATE_ENGINE_TEST_ARGS=--workspace", gate_plan.render_shell(plan("scripts/x.py")))

    def test_text_names_the_deferred_areas(self):
        text = gate_plan.render_text(plan("docs/a.md"), "fast")
        self.assertIn("after the merge only: e2e editor_perf timing", text)

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

    def test_green_or_unrecorded_main_is_silent(self):
        self.assertIsNone(gate_plan.main_health_warning(self.health({"sha": "a", "result": "pass"})))
        self.assertIsNone(gate_plan.main_health_warning(self.health(None)))


if __name__ == "__main__":
    unittest.main()
