"""Regression tests for the dead public Swift declaration check."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from check_dead_public_swift import ALLOWLIST, candidates, problems
from dead_declarations import ROOT

LIBRARY = '''
import Foundation

public struct Used {
    public func usedByTheMac() {}
    public func usedByAShop() {}
    public func usedByATest() {}
    public func usedByTheLibrary() {}
    public func dead() {}
    public static func deadStatic() {}
    public var deadProperty: Int { 0 }
    public let deadConstant = 1
    func internalOnly() {}
    public func onlyACommentNamesMe() {}
    public func onlyALineCommentNamesMe() {} // onlyALineCommentNamesMe
    public func calledAfterAUrl() {}
    public var body: some View { EmptyView() }
}

// onlyACommentNamesMe
/* onlyACommentNamesMe
   spans lines */
public final class DeadClass {}
public protocol DeadProtocol {}
public typealias DeadAlias = Int
public enum DeadEnum { case one }
@MainActor
public func deadAttributed() {}
public func caller() {
    usedByTheLibrary()
    let address = "https://example.com"; calledAfterAUrl()
}
'''


class DeadPublicSwiftTests(unittest.TestCase):
    def fixture(self) -> Path:
        root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        for relative in (
            "apps/shared/Sources/Client",
            "apps/shared/Tests/ClientTests",
            "apps/mac/Sources",
            "apps/ios/Sources",
            "contracts/swift/Sources/Protocol",
        ):
            (root / relative).mkdir(parents=True)
        (root / "apps/shared/Sources/Client/Library.swift").write_text(LIBRARY)
        (root / "apps/mac/Sources/App.swift").write_text("func run() { Used().usedByTheMac() }\n")
        (root / "apps/ios/Sources/App.swift").write_text("func run() { Used().usedByAShop() }\n")
        (root / "apps/shared/Tests/ClientTests/ClientTests.swift").write_text(
            "func test() { Used().usedByATest() }\n"
        )
        (root / "contracts/swift/Sources/Protocol/Generated.swift").write_text(
            "func generated() { Used().usedByTheLibrary() }\n"
        )
        return root

    def names(self, root: Path) -> list[str]:
        return [name for _, _, name in candidates(root)]

    def test_lists_only_public_declarations_nothing_refers_to(self):
        self.assertEqual(
            self.names(self.fixture()),
            [
                "dead",
                "deadStatic",
                "deadProperty",
                "deadConstant",
                "onlyACommentNamesMe",
                "onlyALineCommentNamesMe",
                "DeadClass",
                "DeadProtocol",
                "DeadAlias",
                "DeadEnum",
                "deadAttributed",
                "caller",
            ],
        )

    def test_a_reference_from_a_test_the_mac_the_phone_or_the_generated_code_keeps_a_name(self):
        names = self.names(self.fixture())
        for used in ("usedByTheMac", "usedByAShop", "usedByATest", "usedByTheLibrary"):
            self.assertNotIn(used, names)

    def test_a_name_after_a_url_string_on_the_same_line_is_still_a_reference(self):
        self.assertNotIn("calledAfterAUrl", self.names(self.fixture()))

    def test_framework_names_and_internal_declarations_are_never_listed(self):
        names = self.names(self.fixture())
        self.assertNotIn("body", names)
        self.assertNotIn("internalOnly", names)

    def test_a_case_pattern_is_a_reference_to_the_enum_case_it_names(self):
        root = self.fixture()
        (root / "apps/shared/Sources/Client/Library.swift").write_text(
            "public enum Mode { case on, off }\n"
            "public func isOn(_ mode: Mode) -> Bool {\n"
            "    switch mode {\n    case .on: return true\n    case .off: return false\n    }\n}\n"
        )
        (root / "apps/mac/Sources/App.swift").write_text("func run() { _ = isOn(.on) }\n")
        self.assertEqual(self.names(root), [])

    def test_reports_the_line_of_the_declaration(self):
        found = {name: line for _, line, name in candidates(self.fixture())}
        self.assertEqual(found["dead"], 9)

    def test_the_allowlist_keeps_a_reviewed_entry_and_rejects_stale_ones(self):
        root = self.fixture()
        everything = {
            f"apps/shared/Sources/Client/Library.swift::{name}": "reason"
            for name in self.names(root)
        }
        self.assertEqual(problems(root, everything), [])
        stale = {**everything, "apps/shared/Sources/Client/Library.swift::gone": "reason"}
        self.assertEqual(
            problems(root, stale),
            [
                "apps/shared/Sources/Client/Library.swift::gone: "
                "allowlist entry matches no dead public declaration; remove it"
            ],
        )
        blank = {**everything, "apps/shared/Sources/Client/Library.swift::dead": " "}
        self.assertEqual(
            problems(root, blank),
            ["apps/shared/Sources/Client/Library.swift::dead: allowlist entry needs a reason"],
        )

    def test_an_unlisted_candidate_names_the_fix(self):
        messages = problems(self.fixture(), {})
        self.assertEqual(len(messages), 12)
        self.assertIn("make it internal", messages[0])

    def test_the_repository_has_no_dead_public_declaration(self):
        allowlist = json.loads(ALLOWLIST.read_text(encoding="utf-8"))
        self.assertEqual(problems(ROOT, allowlist), [])


if __name__ == "__main__":
    unittest.main()
