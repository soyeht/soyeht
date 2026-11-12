import copy
import json
import tempfile
import unittest
from pathlib import Path

import compat
from generate import ROOT, validate_open_enums, validate_protocol_versions

LIVE = json.loads((ROOT / "contracts/protocol.json").read_text())


def contract():
    """A small contract: `Ask` goes to the engine, `Answer` comes back, `Mixed` goes both ways."""
    return {
        "x-protocolVersion": {"min": 1, "current": 1},
        "x-methods": {"ask": {"params": {"$ref": "#/$defs/Ask"}, "result": {"$ref": "#/$defs/Answer"}}},
        "x-events": {"told": {"params": {"$ref": "#/$defs/Told"}, "durable": True}},
        "x-errorCodes": {"not_found": -32001},
        "x-framing": {"endianness": "big"},
        "x-limits": {"JSON_BYTES": 100},
        "$defs": {
            "Ask": {
                "type": "object",
                "properties": {
                    "name": {"type": "string", "maxLength": 10},
                    "mode": {"$ref": "#/$defs/Mode"},
                    "note": {"type": "string"},
                },
                "required": ["name"],
                "additionalProperties": True,
            },
            "Answer": {
                "type": "object",
                "properties": {"count": {"type": "integer", "minimum": 0}, "kind": {"$ref": "#/$defs/Kind"}},
                "required": ["count"],
                "additionalProperties": True,
            },
            "Told": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"], "additionalProperties": True},
            "Mode": {"type": "string", "enum": ["fast", "slow"]},
            "Kind": {"type": "string", "enum": ["a", "b"]},
        },
    }


def changed(edit):
    new = copy.deepcopy(contract())
    edit(new)
    return compat.problems(contract(), new)


ALLOWED = {
    "a new method": lambda c: c["x-methods"].update(more={"params": {"$ref": "#/$defs/Ask"}, "result": {"$ref": "#/$defs/Answer"}}),
    "a new event": lambda c: c["x-events"].update(other={"params": {"$ref": "#/$defs/Told"}, "durable": False}),
    "a new error code": lambda c: c["x-errorCodes"].update(late=-32099),
    "a new type": lambda c: c["$defs"].update(Extra={"type": "object", "properties": {}, "required": []}),
    "a new optional property of params": lambda c: c["$defs"]["Ask"]["properties"].update(extra={"type": "string"}),
    "a new optional property of a result": lambda c: c["$defs"]["Answer"]["properties"].update(extra={"type": "string"}),
    "a new required property of a result": lambda c: (
        c["$defs"]["Answer"]["properties"].update(extra={"type": "string"}),
        c["$defs"]["Answer"]["required"].append("extra"),
    ),
    "a new enum value the engine reads": lambda c: c["$defs"]["Mode"]["enum"].append("later"),
    "a looser bound on params": lambda c: c["$defs"]["Ask"]["properties"]["name"].update(maxLength=20),
    "a tighter bound on a result": lambda c: c["$defs"]["Answer"]["properties"]["count"].update(minimum=1),
    "a larger limit": lambda c: c["x-limits"].update(JSON_BYTES=200),
    "a new open enum value": lambda c: (c.update({"x-openEnums": ["Kind"]}), c["$defs"]["Kind"]["enum"].append("c")),
}

FORBIDDEN = {
    "a removed method": lambda c: c["x-methods"].pop("ask"),
    "a removed event": lambda c: c["x-events"].pop("told"),
    "a method with another result": lambda c: c["x-methods"]["ask"].update(result={"$ref": "#/$defs/Told"}),
    "a removed type": lambda c: c["$defs"].pop("Told"),
    "a removed property": lambda c: c["$defs"]["Answer"]["properties"].pop("kind"),
    "a renamed property": lambda c: c["$defs"]["Ask"]["properties"].update(title=c["$defs"]["Ask"]["properties"].pop("note")),
    "a changed property type": lambda c: c["$defs"]["Answer"]["properties"]["count"].update(type="string"),
    "a property that became required": lambda c: c["$defs"]["Ask"]["required"].append("note"),
    "a property that became optional": lambda c: c["$defs"]["Answer"]["required"].remove("count"),
    "a new required property of params": lambda c: (
        c["$defs"]["Ask"]["properties"].update(extra={"type": "string"}),
        c["$defs"]["Ask"]["required"].append("extra"),
    ),
    "a new enum value the engine sends": lambda c: c["$defs"]["Kind"]["enum"].append("c"),
    "a removed enum value": lambda c: c["$defs"]["Mode"]["enum"].remove("slow"),
    "a tighter bound on params": lambda c: c["$defs"]["Ask"]["properties"]["name"].update(maxLength=5),
    "a looser bound on a result": lambda c: c["$defs"]["Answer"]["properties"]["count"].update(minimum=-5),
    "an error code that is gone": lambda c: c["x-errorCodes"].pop("not_found"),
    "an error code with another number": lambda c: c["x-errorCodes"].update(not_found=-32002),
    "a smaller limit": lambda c: c["x-limits"].update(JSON_BYTES=50),
    "unknown properties no longer ignored": lambda c: c["$defs"]["Ask"].update(additionalProperties=False),
}


class AdditiveTests(unittest.TestCase):
    def test_every_addition_is_allowed(self):
        for name, edit in ALLOWED.items():
            with self.subTest(name):
                self.assertEqual(changed(edit), [])

    def test_every_break_is_named(self):
        for name, edit in FORBIDDEN.items():
            with self.subTest(name):
                self.assertTrue(changed(edit), "the break went through")

    def test_a_prose_change_is_not_a_change(self):
        new = contract()
        new["$defs"]["Ask"]["description"] = "new words"
        self.assertEqual(compat.problems(contract(), new), [])
        self.assertEqual(compat.surface(contract()), compat.surface(new))

    def test_a_type_that_both_sides_read_takes_no_new_enum_value(self):
        both = contract()
        both["x-methods"]["ask"]["params"] = {"$ref": "#/$defs/Mixed"}
        both["$defs"]["Mixed"] = {"type": "object", "properties": {"mode": {"$ref": "#/$defs/Mode"}}, "required": []}
        both["x-events"]["told"]["params"] = {"$ref": "#/$defs/Mixed"}
        grown = copy.deepcopy(both)
        grown["$defs"]["Mode"]["enum"].append("later")
        self.assertTrue(compat.problems(both, grown))

    def test_an_open_enum_cannot_be_closed_again(self):
        opened = contract()
        opened["x-openEnums"] = ["Kind"]
        self.assertEqual(compat.problems(contract(), opened), [])
        self.assertTrue(compat.problems(opened, contract()))

    def test_the_live_contract_is_additive_against_itself(self):
        self.assertEqual(compat.problems(LIVE, LIVE), [])


class ReleasedTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)
        self.root = Path(self.folder.name)

    def test_nothing_is_checked_before_the_first_release(self):
        self.assertEqual(compat.released_problems(self.root, contract()), [])
        self.assertIn("is missing", compat.missing_snapshot(self.root, contract()))

    def test_a_snapshot_records_the_release_and_is_never_replaced(self):
        compat.write_snapshot(self.root, contract(), "0.1.0")
        self.assertEqual(json.loads((self.root / compat.RELEASED / "index.json").read_text()), {"1": {"productVersion": "0.1.0"}})
        self.assertIsNone(compat.missing_snapshot(self.root, contract()))
        self.assertIsNone(compat.write_snapshot(self.root, contract(), "0.1.0"))
        grown = contract()
        grown["x-errorCodes"]["late"] = -32099
        with self.assertRaises(ValueError):
            compat.write_snapshot(self.root, grown, "0.2.0")

    def test_a_change_after_a_release_needs_a_higher_current(self):
        compat.write_snapshot(self.root, contract(), "0.1.0")
        grown = contract()
        grown["x-errorCodes"]["late"] = -32099
        self.assertIn("raise x-protocolVersion.current", "\n".join(compat.released_problems(self.root, grown)))
        grown["x-protocolVersion"]["current"] = 2
        self.assertEqual(compat.released_problems(self.root, grown), [])

    def test_a_break_is_found_against_the_snapshot(self):
        compat.write_snapshot(self.root, contract(), "0.1.0")
        broken = contract()
        broken["x-methods"].pop("ask")
        broken["x-protocolVersion"]["current"] = 2
        found = compat.released_problems(self.root, broken)
        self.assertEqual(len(found), 1)
        self.assertIn("protocol-v1.json", found[0])

    def test_a_break_by_raising_min_deletes_the_old_snapshots(self):
        compat.write_snapshot(self.root, contract(), "0.1.0")
        broken = contract()
        broken["x-methods"].pop("ask")
        broken["x-protocolVersion"] = {"min": 2, "current": 2, "minReason": "ask is replaced by a method that"}
        self.assertIn("delete it", "\n".join(compat.released_problems(self.root, broken)))
        compat.snapshot_path(self.root, 1).unlink()
        self.assertEqual(compat.released_problems(self.root, broken), [])


class OpenEnumTests(unittest.TestCase):
    def listed(self, *names):
        return contract() | {"x-openEnums": list(names)}

    def test_an_enum_the_engine_sends_can_be_open(self):
        validate_open_enums(self.listed("Kind"))

    def test_only_an_enum_can_be_open(self):
        for names in (("Answer",), ("Missing",), ("Kind", "Kind")):
            with self.subTest(names), self.assertRaises(ValueError):
                validate_open_enums(self.listed(*names))

    def test_an_enum_the_engine_reads_from_a_client_stays_closed(self):
        with self.assertRaises(ValueError):
            validate_open_enums(self.listed("Mode"))

    def test_a_value_called_unknown_would_clash_with_the_catch_all(self):
        opened = self.listed("Kind")
        opened["$defs"]["Kind"]["enum"].append("unknown")
        with self.assertRaises(ValueError):
            validate_open_enums(opened)

    def test_the_live_contract_lists_only_valid_open_enums(self):
        validate_open_enums(LIVE)


class RangeTests(unittest.TestCase):
    def test_the_range_is_whole_numbers_with_min_at_most_current(self):
        validate_protocol_versions({"min": 1, "current": 3})
        for bad in ({"min": 0, "current": 1}, {"min": 3, "current": 2}, {"min": 1}, {"min": "1", "current": 1}, {"min": 1, "current": 1, "x": 1}):
            with self.subTest(bad), self.assertRaises(ValueError):
                validate_protocol_versions(bad)

    def test_a_min_above_one_says_why(self):
        with self.assertRaises(ValueError):
            validate_protocol_versions({"min": 2, "current": 2})
        validate_protocol_versions({"min": 2, "current": 2, "minReason": "removed X"})


if __name__ == "__main__":
    unittest.main()
