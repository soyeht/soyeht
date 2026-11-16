"""The vector files of the voice loop are what contracts/voice_vectors.py writes."""

import json
import unittest
from pathlib import Path

import voice_vectors
import voice_vectors_turns

SCHEMA = json.loads((Path(__file__).resolve().parent / "protocol.json").read_text())


class VoiceVectorTests(unittest.TestCase):
    def test_the_files_are_what_the_script_writes(self):
        self.assertEqual(voice_vectors.main(["--check"]), 0)

    def test_every_phase_meets_every_kind_of_input(self):
        turns = json.loads(voice_vectors_turns.TEXT)
        phases = set(turns["reach"])
        kinds = {
            "start", "stop", "frames", "endOfTurn", "transcript", "sentenceStarted",
            "replyFinished", "interrupt",
        }
        met = {(pair["reach"], pair["steps"][-1]["input"]) for pair in turns["pairs"]}
        self.assertEqual(met, {(phase, kind) for phase in phases for kind in kinds})

    def test_the_cases_are_written_for_the_policy_the_vectors_name(self):
        turns = json.loads(voice_vectors_turns.TEXT)
        self.assertEqual(turns["policy"], "x-clientPolicy.voice")
        self.assertIn("voice", SCHEMA["x-clientPolicy"])
        self.assertNotIn("defaults", turns)


if __name__ == "__main__":
    unittest.main()
