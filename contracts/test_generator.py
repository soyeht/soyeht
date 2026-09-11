import copy
import json
import unittest
from pathlib import Path

from generate import validate_schema

ROOT = Path(__file__).resolve().parent


class SchemaTests(unittest.TestCase):
    def setUp(self):
        self.schema = json.loads((ROOT / "protocol.json").read_text())

    def test_unknown_keywords_and_references_fail_closed(self):
        validate_schema(self.schema)
        for patch in ({"unknownKeyword": True}, {"$ref": "#/$defs/Missing"}):
            changed = copy.deepcopy(self.schema)
            changed["$defs"]["Id"].update(patch)
            with self.assertRaises(ValueError):
                validate_schema(changed)

    def test_every_method_and_event_has_a_wire_vector(self):
        names = {v["name"] for v in json.loads((ROOT / "vectors/messages.json").read_text())}
        for name in self.schema["x-methods"]:
            self.assertIn("request:" + name, names)
            self.assertIn("response:" + name, names)
        for name in self.schema["x-events"]:
            self.assertIn("event:" + name, names)

    def test_mutations_require_an_idempotency_key(self):
        for name, method in self.schema["x-methods"].items():
            if method["mutation"]:
                definition = self.schema["$defs"][method["params"]["$ref"].split("/")[-1]]
                self.assertIn("idempotencyKey", definition["required"], name)


if __name__ == "__main__":
    unittest.main()
