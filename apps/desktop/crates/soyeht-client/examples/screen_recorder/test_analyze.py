"""Tests of analyze.py: uv run python -m unittest test_analyze (from this folder)."""

import json
import re
import tempfile
import unittest
from pathlib import Path

import analyze

PROMPT = re.compile(analyze.PROMPT_END)


def change(t, seq, line, col, alt=False, hidden=False, above=(), below=()):
    return {
        "v": 1, "kind": "change", "t": t, "m": t, "pane": "p", "seq": seq, "alt": alt,
        "cursor": {"row": 0, "col": col, "hidden": hidden}, "line": line,
        "above": list(above), "below": list(below), "outBytes": 1,
    }


class StateTest(unittest.TestCase):
    def state(self, line, col, **fields):
        return analyze.state_of(change(0, 1, line, col, **fields), PROMPT)

    def test_a_shell_prompt_is_empty_until_something_is_typed_after_it(self):
        self.assertEqual(self.state("user@mac ~ %", 13), "empty")
        self.assertEqual(self.state("user@mac ~ % git", 16), "typing")
        self.assertEqual(self.state("$ ls", 4), "typing")

    def test_the_input_box_of_a_tui_is_read_through_its_border(self):
        self.assertEqual(self.state("│ >                  │", 4), "empty")
        self.assertEqual(self.state("│ > fix bug          │", 11), "typing")

    def test_text_to_the_right_of_the_cursor_is_still_input(self):
        self.assertEqual(self.state("$ ls -la", 2), "typing")

    def test_output_the_alternate_screen_and_a_hidden_cursor_are_not_an_input_line(self):
        self.assertEqual(self.state("file.txt", 8), "other")
        self.assertEqual(self.state("", 0), "other")
        self.assertEqual(self.state("-- INSERT --", 12, alt=True), "alt")
        self.assertEqual(self.state("> ", 2, hidden=True), "other")

    def test_a_wide_character_counts_two_cells(self):
        self.assertEqual(self.state("日本 > a", 7), "typing")
        self.assertEqual(self.state("日本 >", 6), "empty")


class TransitionTest(unittest.TestCase):
    def test_the_moments_the_line_emptied_and_filled_with_how_long_the_screen_stayed_still(self):
        records = [
            change(1000, 1, "READY>", 7),
            change(4000, 2, "READY> a", 8),
            change(4200, 3, "READY> ab", 9),
            change(9200, 4, "READY>", 7),
            change(9400, 5, "READY> x", 8),
            change(9600, 6, "x: not found", 12),
        ]
        found = analyze.transitions(records, PROMPT)
        self.assertEqual(
            [(item["meaning"], item["seq"], item["still"]) for item in found],
            [
                ("prompt_back", 1, 3.0),
                ("typing_began", 2, 0.2),
                ("cleared", 4, 0.2),
                ("typing_began", 5, 0.2),
            ],
        )

    def test_leaving_the_prompt_and_taking_the_screen_are_told_apart_from_clearing(self):
        records = [
            change(0, 1, "READY>", 7),
            change(100, 2, "", 0),
            change(200, 3, "READY>", 7),
            change(300, 4, "~", 1, alt=True),
        ]
        found = analyze.transitions(records, PROMPT)
        self.assertEqual(
            [item["meaning"] for item in found],
            ["prompt_back", "left_the_prompt", "prompt_back", "took_the_screen"],
        )
        self.assertIsNone(found[-1]["still"])


class FilesTest(unittest.TestCase):
    def test_rotated_files_are_read_with_the_live_one_in_time_order_and_a_cut_line_is_skipped(self):
        with tempfile.TemporaryDirectory() as folder:
            folder = Path(folder)
            (folder / "p.1.jsonl").write_text(json.dumps(change(1, 1, "a", 1)) + "\n")
            (folder / "p.2.jsonl").write_text(json.dumps(change(2, 2, "b", 1)) + "\n")
            (folder / "p.jsonl").write_text(json.dumps(change(3, 3, "c", 1)) + "\n{\"kind\": \"cha")
            (folder / "other.jsonl").write_text(json.dumps(change(0, 9, "z", 1)) + "\n")
            for name in ("p.jsonl", "p.1.jsonl"):
                records = analyze.read_records(folder / name)
                self.assertEqual([r["line"] for r in records], ["a", "b", "c"])


if __name__ == "__main__":
    unittest.main()
