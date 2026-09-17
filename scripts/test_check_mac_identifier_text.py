import unittest

from check_mac_identifier_text import violations


class MacIdentifierTextTests(unittest.TestCase):
    def test_rejects_direct_identifier_interpolation(self) -> None:
        source = r'''
Text("port \(server.port) · pid \(server.pid)")
Text("\(scene.ordinal + 1)")
Text("p. \(page(for: scene))")
Text("Attempt \(attempt.n)")
Text("\(match.range.start.line):\(match.range.start.column)")
Text("\(slide.index + 1)")
Text("line \(hunk.line)")
Text("Ln \(selectedPosition?.line), Col \(selectedPosition?.column)")
Text("r\(version.revision)")
Text("\(viewport.width) × \(viewport.height)")
'''
        self.assertEqual(violations(source), list(range(2, 12)))

    def test_allows_explicit_identifier_interpolation(self) -> None:
        source = r'''
Text("port \(identifier: server.port) · pid \(identifier: server.pid)")
Text("\(identifier: scene.ordinal + 1)")
Text("p. \(identifier: page(for: scene))")
Text("Attempt \(identifier: attempt.n)")
Text("\(identifier: match.range.start.line):\(identifier: match.range.start.column)")
Text("\(identifier: slide.index + 1)")
Text("line \(identifier: hunk.line)")
Text("Ln \(identifier: selectedPosition?.line), Col \(identifier: selectedPosition?.column)")
Text("r\(identifier: version.revision)")
Text("\(identifier: viewport.width) × \(identifier: viewport.height)")
'''
        self.assertEqual(violations(source), [])

    def test_allows_localized_quantities_and_non_text_addresses(self) -> None:
        source = r'''
Text("\(tasks.count) tasks")
Text("\(entry.bytes) bytes")
Text("Preview has \(viewport.file.lineCount) lines")
let address = "http://127.0.0.1:\(server.port)"
'''
        self.assertEqual(violations(source), [])

    def test_rejects_identifier_labels_even_when_the_value_is_renamed(self) -> None:
        source = r'''
Text("port \(value)")
Text("pid \(value)")
Text("line \(value)")
Text("r\(value)")
'''
        self.assertEqual(violations(source), [2, 3, 4, 5])


if __name__ == "__main__":
    unittest.main()
