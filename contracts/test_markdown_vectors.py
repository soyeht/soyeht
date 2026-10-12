"""The Markdown vectors must be well formed before any client is asked to pass them."""

import json
import unittest
from pathlib import Path

VECTORS = json.loads((Path(__file__).parent / "vectors/markdown.json").read_text())
RUN_KEYS = {"text", "bold", "italic", "strike", "code", "link"}
SCHEMES = ("http://", "https://", "mailto:")


def walk(blocks):
    """Every block, nested ones included."""
    for block in blocks:
        yield block
        yield from walk(block.get("blocks", []))
        for item in block.get("items", []):
            yield from walk(item)


def inline_lists(block):
    if "inlines" in block:
        yield block["inlines"]
    for cell in block.get("header", []):
        yield cell
    for row in block.get("rows", []):
        yield from row


class MarkdownVectorTests(unittest.TestCase):
    def test_names_are_unique_and_every_vector_is_complete(self):
        names = [vector["name"] for vector in VECTORS]
        self.assertEqual(len(names), len(set(names)))
        for vector in VECTORS:
            self.assertEqual(set(vector), {"name", "markdown", "streaming", "blocks"})
            self.assertIsInstance(vector["streaming"], bool)

    def test_blocks_use_only_the_documented_shapes(self):
        shapes = {
            "paragraph": {"type", "inlines"},
            "heading": {"type", "level", "inlines"},
            "code": {"type", "language", "text"},
            "quote": {"type", "blocks"},
            "list": {"type", "ordered", "start", "items"},
            "table": {"type", "header", "rows"},
            "rule": {"type"},
        }
        for vector in VECTORS:
            for block in walk(vector["blocks"]):
                self.assertEqual(set(block), shapes[block["type"]], vector["name"])

    def test_runs_are_merged_and_links_are_allowed_addresses(self):
        for vector in VECTORS:
            for block in walk(vector["blocks"]):
                for line in inline_lists(block):
                    previous = None
                    for run in line:
                        self.assertLessEqual(set(run), RUN_KEYS, vector["name"])
                        self.assertTrue(run["text"], vector["name"])
                        style = {k: v for k, v in run.items() if k != "text"}
                        self.assertNotEqual(style, previous, f"unmerged runs in {vector['name']}")
                        previous = style
                        link = run.get("link")
                        self.assertTrue(link is None or link.startswith(SCHEMES), vector["name"])

    def test_every_construct_has_a_vector(self):
        kinds = {block["type"] for vector in VECTORS for block in walk(vector["blocks"])}
        self.assertEqual(kinds, {"paragraph", "heading", "code", "quote", "list", "table", "rule"})
        self.assertTrue(any(vector["streaming"] for vector in VECTORS))


if __name__ == "__main__":
    unittest.main()
