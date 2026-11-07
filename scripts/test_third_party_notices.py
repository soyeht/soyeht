"""Tests of the third-party notices: the license rules, the sources and the file they render."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import third_party_licenses as licenses
import third_party_notices as notices
import third_party_sources as sources

POLICY = licenses.load_policy()
GPL_TEXT = "GNU GENERAL PUBLIC LICENSE\nVersion 3, 29 June 2007\n" + "Everyone is permitted to copy and distribute verbatim copies of this license document. " * 3
MIT_TEXT = (
    "MIT License\n\nCopyright (c) 2020 Jane Doe\n\nPermission is hereby granted, free of charge, to any "
    "person obtaining a copy of this software to deal in the Software without restriction, subject to the "
    "following conditions: the notice above shall be included in all copies.\n"
)
APACHE_TEXT = (
    "                                 Apache License\n                           Version 2.0, January 2004\n"
    "TERMS AND CONDITIONS\n   END OF TERMS AND CONDITIONS\n\n   APPENDIX: How to apply the Apache License to "
    "your work.\n   Copyright [yyyy] [name of copyright owner]\n   See the License for the specific language "
    "governing permissions and limitations under the License.\n\nCopyright 2021 Acme\n"
)


class ExpressionTests(unittest.TestCase):
    def test_or_needs_one_allowed_side_and_and_needs_both(self):
        self.assertEqual(licenses.usable_choices("MIT OR GPL-3.0-or-later", POLICY), [frozenset({"MIT"})])
        self.assertEqual(licenses.usable_choices("Apache-2.0 OR GPL-2.0-only", POLICY), [frozenset({"Apache-2.0"})])
        self.assertEqual(licenses.usable_choices("MIT AND GPL-3.0-or-later", POLICY), [])
        self.assertEqual(licenses.usable_choices("GPL-3.0-or-later", POLICY), [])
        self.assertEqual(licenses.usable_choices("LGPL-2.1-or-later", POLICY), [])

    def test_the_old_slash_and_parentheses_and_with_are_read(self):
        both = licenses.usable_choices("(MIT OR Apache-2.0) AND Unicode-3.0", POLICY)
        self.assertEqual(both[0], frozenset({"MIT", "Unicode-3.0"}))
        self.assertEqual(len(both), 2)
        self.assertEqual(len(licenses.usable_choices("MIT/Apache-2.0", POLICY)), 2)
        self.assertEqual(
            licenses.usable_choices("Apache-2.0 WITH LLVM-exception OR MIT", POLICY)[0], frozenset({"MIT"})
        )
        self.assertEqual(
            licenses.usable_choices("Apache-2.0 WITH LLVM-exception", POLICY), [frozenset({"Apache-2.0 WITH LLVM-exception"})]
        )

    def test_the_most_preferred_alternative_comes_first(self):
        self.assertEqual(licenses.usable_choices("Apache-2.0 OR MIT", POLICY)[0], frozenset({"MIT"}))

    def test_an_expression_that_cannot_be_read_is_an_error(self):
        for bad in ("MIT OR", "(MIT", "MIT AND AND ISC", ""):
            with self.assertRaises(licenses.LicenseError, msg=bad):
                licenses.usable_choices(bad, POLICY)


class FileTests(unittest.TestCase):
    def test_a_license_text_is_recognised_by_its_words(self):
        self.assertEqual(licenses.classify(MIT_TEXT), "MIT")
        self.assertEqual(licenses.classify(APACHE_TEXT), "Apache-2.0")
        self.assertEqual(licenses.classify("GNU GENERAL PUBLIC LICENSE\nVersion 3, 29 June 2007"), "GPL-3.0-or-later")
        self.assertIsNone(licenses.classify("Use it however you like."))

    def test_the_alternative_the_package_ships_is_the_one_elected(self):
        files = {"LICENSE-APACHE": APACHE_TEXT}
        chosen, kept = licenses.elect("MIT OR Apache-2.0", files, POLICY)
        self.assertEqual((chosen, kept), (frozenset({"Apache-2.0"}), ["LICENSE-APACHE"]))
        files["LICENSE-MIT"] = MIT_TEXT
        chosen, kept = licenses.elect("MIT OR Apache-2.0", files, POLICY)
        self.assertEqual((chosen, kept), (frozenset({"MIT"}), ["LICENSE-MIT"]))

    def test_a_notice_and_an_unknown_text_are_always_kept_and_a_gpl_file_of_an_unchosen_side_is_not(self):
        files = {"LICENSE-MIT": MIT_TEXT, "LICENSE-GPL": GPL_TEXT, "NOTICE": "Acme", "LICENSE": "Custom."}
        _, kept = licenses.elect("MIT OR GPL-3.0-or-later", files, POLICY)
        self.assertEqual(kept, ["LICENSE", "LICENSE-MIT", "NOTICE"])

    def test_an_expression_with_nothing_allowed_is_an_error(self):
        with self.assertRaises(licenses.LicenseError):
            licenses.elect("GPL-3.0-or-later", {}, POLICY)

    def test_a_missing_text_is_the_standard_one(self):
        chosen, kept = licenses.elect("MIT", {}, POLICY)
        self.assertEqual(licenses.missing_texts(chosen, {}, kept), ["MIT"])
        self.assertIn("Permission is hereby granted", licenses.spdx_text("MIT"))

    def test_the_copyright_lines_leave_the_terms(self):
        rights, parts = licenses.explode("MIT", MIT_TEXT)
        self.assertEqual(rights, ["Copyright (c) 2020 Jane Doe"])
        self.assertTrue(parts[0][1].startswith("Permission is hereby granted"))

    def test_a_text_with_other_words_before_its_terms_stays_whole(self):
        odd = "Portions are by someone else.\nCopyright (c) 2020 Jane\n" + MIT_TEXT[MIT_TEXT.index("Permission") :]
        self.assertEqual(licenses.explode("MIT", odd), ([], [("MIT", odd)]))

    def test_the_apache_terms_are_kept_once_and_the_appendix_goes(self):
        rights, parts = licenses.explode("Apache-2.0", APACHE_TEXT)
        self.assertEqual(rights, ["Copyright 2021 Acme"])
        self.assertEqual([label for label, _ in parts], ["Apache-2.0"])
        self.assertNotIn("APPENDIX", parts[0][1])
        self.assertTrue(parts[0][1].rstrip().endswith("END OF TERMS AND CONDITIONS"))


def package(folder, name, license, files, version="1.0.0", source="registry+x"):
    directory = Path(folder) / name
    directory.mkdir(parents=True, exist_ok=True)
    for file, text in files.items():
        (directory / file).write_text(text)
    return {
        "id": f"{name} {version}", "name": name, "version": version, "license": license, "source": source,
        "manifest_path": str(directory / "Cargo.toml"), "authors": ["Jane <j@example.com>"], "repository": "https://example.com/" + name,
    }


def fake_metadata(packages, edges, members=("app",)):
    """`cargo metadata` output for `packages` with `edges` = {name: [(dependency, kind)]}."""
    ids = {p["name"]: p["id"] for p in packages}
    nodes = [
        {"id": p["id"], "deps": [{"pkg": ids[d], "dep_kinds": [{"kind": k}]} for d, k in edges.get(p["name"], [])]}
        for p in packages
    ]
    return {"packages": packages, "workspace_members": [ids[m] for m in members], "resolve": {"nodes": nodes}}


class CargoTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)

    def metadata(self, third_party, edges=None):
        app = package(self.directory.name, "app", "Apache-2.0", {}, source=None)
        return fake_metadata([app, *third_party], edges or {"app": [(p["name"], None) for p in third_party]})

    def test_only_the_normal_dependencies_of_the_members_are_shipped(self):
        a = package(self.directory.name, "shipped", "MIT", {"LICENSE": MIT_TEXT})
        b = package(self.directory.name, "tests-only", "MIT", {"LICENSE": MIT_TEXT})
        c = package(self.directory.name, "deep", "MIT", {"LICENSE": MIT_TEXT})
        metadata = self.metadata([a, b, c], {"app": [("shipped", None), ("tests-only", "dev")], "shipped": [("deep", None)]})
        self.assertEqual([p["name"] for p in sources.shipped_packages(metadata)], ["deep", "shipped"])

    def test_a_package_of_this_repository_is_not_third_party(self):
        own = package(self.directory.name, "own", "Apache-2.0", {}, source=None)
        metadata = self.metadata([own])
        self.assertEqual(sources.shipped_packages(metadata), [])

    def test_a_package_gets_its_elected_texts_and_copyright(self):
        pkg = package(self.directory.name, "lib", "MIT OR Apache-2.0", {"LICENSE-MIT": MIT_TEXT, "LICENSE-APACHE": APACHE_TEXT})
        component = sources.cargo_component(pkg, POLICY)
        self.assertEqual(component.copyright, ["Copyright (c) 2020 Jane Doe"])
        self.assertEqual([t.label for t in component.texts], ["MIT"])

    def test_a_package_without_a_license_file_takes_the_standard_text_and_names_its_authors(self):
        pkg = package(self.directory.name, "bare", "MIT", {})
        component = sources.cargo_component(pkg, POLICY)
        self.assertEqual([t.label for t in component.texts], ["MIT"])
        self.assertEqual(component.copyright, ["Copyright (c) Jane (named by the package metadata)"])

    def test_a_link_that_the_archive_lost_is_not_a_license(self):
        pkg = package(self.directory.name, "linked", "MIT OR Apache-2.0", {"LICENSE-APACHE": "../LICENSE-APACHE", "LICENSE-MIT": MIT_TEXT})
        self.assertEqual(list(sources.read_license_files(Path(pkg["manifest_path"]).parent)), ["LICENSE-MIT"])

    def test_a_package_that_declares_no_license_needs_a_clarification(self):
        pkg = package(self.directory.name, "mystery", None, {"LICENSE": MIT_TEXT})
        with self.assertRaises(sources.SourceError):
            sources.cargo_component(pkg, POLICY)

    def test_a_gpl_package_fails_unless_the_policy_lists_it_with_a_reason(self):
        gpl = package(self.directory.name, "viral", "GPL-3.0-or-later", {"LICENSE": GPL_TEXT})
        with self.assertRaisesRegex(sources.SourceError, "viral"):
            notices.collect(POLICY, metadata=lambda w, r: self.metadata([gpl]))
        allowed = licenses.Policy(POLICY.allowed, POLICY.prefer, POLICY.clarify, {("viral", "GPL-3.0-or-later"): "reviewed"})
        found = sources.cargo_component(gpl, allowed)
        self.assertEqual(found.exception, "reviewed")

    def test_an_exception_nobody_uses_fails(self):
        extra = licenses.Policy(POLICY.allowed, POLICY.prefer, POLICY.clarify, {("ghost", "GPL-3.0-or-later"): "once"})
        with self.assertRaisesRegex(sources.SourceError, "exceptions"):
            notices.collect(extra, metadata=lambda w, r: self.metadata([]))

    def test_an_exception_needs_a_reason(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "policy.json"
            path.write_text(json.dumps({"allowed": [], "prefer": [], "clarify": {}, "exceptions": [{"package": "x", "license": "GPL-3.0-only", "reason": " "}]}))
            with self.assertRaises(licenses.LicenseError):
                licenses.load_policy(path)


class RenderTests(unittest.TestCase):
    def component(self, name, label, body, rights=()):
        found = sources.Component("Rust", name, "1.0", "MIT", "https://example.com", {"engine"})
        sources.add_text(found, label, body)
        found.copyright += list(rights)
        return found

    def test_one_text_is_written_once_for_every_package_that_uses_it(self):
        text = render = notices.render([self.component("a", "MIT", MIT_TEXT), self.component("b", "MIT", MIT_TEXT)])
        self.assertEqual(text.count("Permission is hereby granted"), 1)
        self.assertIn("Used by: a 1.0, b 1.0", render)

    def test_the_output_does_not_depend_on_the_order_of_the_input(self):
        one, two = self.component("a", "MIT", MIT_TEXT), self.component("b", "Apache-2.0", APACHE_TEXT)
        self.assertEqual(notices.render([one, two]), notices.render([two, one]))


class RepositoryTests(unittest.TestCase):
    """The data that is committed must describe the lock files of this commit."""

    def test_the_swift_data_matches_every_package_resolved(self):
        components = sources.swift_components(POLICY)
        self.assertEqual({c.name for c in components}, set(sources.resolved_pins()))
        self.assertTrue(all(c.texts for c in components))

    def test_a_moved_pin_is_stale(self):
        pins = sources.resolved_pins()
        data = json.loads(sources.SWIFT_DATA.read_text())
        identity = sorted(pins)[0]
        data["packages"][identity]["revision"] = "0" * 40
        with tempfile.TemporaryDirectory() as folder:
            moved = Path(folder) / "swift.json"
            moved.write_text(json.dumps(data))
            original, sources.SWIFT_DATA = sources.SWIFT_DATA, moved
            try:
                with self.assertRaisesRegex(sources.SourceError, identity):
                    sources.swift_components(POLICY)
            finally:
                sources.SWIFT_DATA = original

    def test_the_gradle_data_matches_the_build_file(self):
        names = {c.name for c in sources.gradle_components(POLICY)}
        self.assertIn("androidx.activity:activity-compose", names)
        declared = sources.gradle_declared()
        self.assertEqual(declared["androidx.camera:camera-core"], "1.4.1")
        self.assertEqual(declared["androidx.compose:compose-bom"], "2024.12.01")
        self.assertEqual(declared["net.java.dev.jna:jna"], "5.14.0")

    def test_the_assets_are_the_fonts_and_icons_of_the_repository(self):
        names = {c.name for c in sources.asset_components(POLICY)}
        self.assertTrue({"Inter", "Lucide icons", "Lobe Icons (provider glyphs)"} <= names)

    def test_every_license_the_policy_prefers_is_allowed_and_has_a_standard_text(self):
        for identifier in POLICY.prefer:
            self.assertIn(identifier, POLICY.allowed)
        for identifier in POLICY.allowed - {"OFL-1.1", "Apache-2.0 WITH LLVM-exception"}:
            licenses.spdx_text(identifier)

    def test_the_policy_holds_no_exception_so_a_gpl_crate_that_comes_back_fails_the_gate(self):
        self.assertEqual(POLICY.exceptions, {})

    def test_no_copyleft_license_is_allowed(self):
        for identifier in POLICY.allowed:
            self.assertNotRegex(identifier, r"GPL|AGPL|SSPL|EUPL|CPAL")

    def test_the_committed_file_is_small_enough_for_the_repository(self):
        if notices.OUTPUT.is_file():
            self.assertLess(notices.OUTPUT.stat().st_size, 700_000)


if __name__ == "__main__":
    unittest.main()
