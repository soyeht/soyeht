"""Writes, or checks, the vector files of the voice loop (contract 20).

    uv run python contracts/voice_vectors.py          write voice-turns.json and speech-text.json
    uv run python contracts/voice_vectors.py --check  fail when a file is not what this script writes

The expected values of the cases are written here by hand, from the rules of
`docs/contracts/20-voice-loop.md`; the script only lays them out, and expands the table of `pairs`
(every phase of the turn machine against every input), so that a file cannot drift from its source.
The cases are written for the values of `x-clientPolicy.voice` in `protocol.json`.
"""

import sys
from pathlib import Path

import voice_vectors_speech
import voice_vectors_turns

VECTORS = Path(__file__).resolve().parent / "vectors"


def outputs():
    return {
        "voice-turns.json": voice_vectors_turns.TEXT,
        "speech-text.json": voice_vectors_speech.TEXT,
    }


def main(argv):
    check = "--check" in argv
    stale = []
    for name, text in outputs().items():
        path = VECTORS / name
        if check:
            if not path.is_file() or path.read_text() != text:
                stale.append(name)
        else:
            path.write_text(text)
    if stale:
        print(f"stale: {', '.join(stale)}: run uv run python contracts/voice_vectors.py", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
