import json
import tempfile
import unittest
from pathlib import Path

import gate_timings

LINES = "a:one\talways\t1.5\t0\nb:two\tengine\t10.25\t1\nbroken line\n"


class TimingTests(unittest.TestCase):
    def setUp(self):
        self.directory = Path(tempfile.mkdtemp())
        self.out = self.directory / "nested/timings.json"
        self.steps_file = self.directory / "steps.tsv"
        self.steps_file.write_text(LINES)

    def test_reads_steps_and_skips_malformed_lines(self):
        steps = gate_timings.read_steps(self.steps_file)
        self.assertEqual(
            steps,
            [
                {"id": "a:one", "area": "always", "seconds": 1.5, "status": 0},
                {"id": "b:two", "area": "engine", "seconds": 10.25, "status": 1},
            ],
        )

    def test_finalize_writes_latest_and_history(self):
        steps = gate_timings.read_steps(self.steps_file)
        data = gate_timings.finalize(steps, "fast", 1, "always engine", self.out, 0)
        written = json.loads(self.out.read_text())
        self.assertEqual(written, data)
        self.assertEqual(written["latest"]["tier"], "fast")
        self.assertEqual(written["latest"]["exit_code"], 1)
        self.assertEqual(written["latest"]["seconds"], 11.75)
        self.assertEqual(written["latest"]["started_at"], "1970-01-01T00:00:00Z")
        self.assertEqual(written["history"][0]["steps"], {"a:one": 1.5, "b:two": 10.25})

    def test_history_is_bounded_and_survives_a_corrupt_file(self):
        steps = gate_timings.read_steps(self.steps_file)
        self.out.parent.mkdir(parents=True)
        self.out.write_text("not json")
        for index in range(gate_timings.HISTORY_LIMIT + 5):
            data = gate_timings.finalize(steps, "full", 0, "", self.out, index)
        self.assertEqual(len(data["history"]), gate_timings.HISTORY_LIMIT)

    def test_slowest_averages_over_the_history(self):
        data = {"history": [{"steps": {"a": 1.0, "b": 9.0}}, {"steps": {"a": 3.0}}]}
        self.assertEqual(gate_timings.slowest(data, 5), [("b", 9.0), ("a", 2.0)])
        self.assertEqual(gate_timings.slowest(data, 1), [("b", 9.0)])


if __name__ == "__main__":
    unittest.main()
