"""Reads a recording of the screen recorder and prints the moments the input line emptied or filled.

    uv run python analyze.py RECORDING.jsonl [--screens] [--json] [--thresholds 1,2,5]

The input line is the line the cursor is on. It is "empty" when what is left of the cursor, once
the decoration around it (blanks and box-drawing characters) is cut off, ends in a prompt glyph and
nothing is right of the cursor; it is "typing" when there is text after the prompt; "other" when
the cursor is on a line with no prompt (command output, a hidden cursor) and "alt" when a
full-screen program has the alternate screen. No program is named anywhere: the same rule reads a
shell prompt and the input box of a harness, and the glyphs are the only guess (`--prompt-end`).

For each time the state changed to or from "empty" it prints the line, the rows around it and how
long the screen then stayed unchanged (`still`), which is the number a rule such as "the input is
empty and has been still for N seconds" has to choose. Rotated files (`pane.1.jsonl`, ...) beside
the one given are read with it.
"""

import argparse
import json
import re
import sys
import unicodedata
from datetime import datetime, timezone
from pathlib import Path

# What surrounds an input line without being part of it: blanks, box-drawing and block characters.
DECORATION = re.compile(r"^[\s─-▟]+|[\s─-▟]+$")
PROMPT_END = r"[>$%#❯›»→λ]$"
ROTATED = re.compile(r"^(?P<pane>.+?)\.(?P<number>\d+)\.jsonl$")

# (from, to) -> what happened, for the changes that involve an empty line.
MEANING = {
    ("typing", "empty"): "cleared",
    ("other", "empty"): "prompt_back",
    ("alt", "empty"): "prompt_back",
    ("empty", "typing"): "typing_began",
    ("empty", "other"): "left_the_prompt",
    ("empty", "alt"): "took_the_screen",
}


def cells(text):
    """The width of each character in terminal cells."""
    return [2 if unicodedata.east_asian_width(c) in "WF" else 1 for c in text]


def split_at_cursor(line, column):
    """The text left of the cursor and the text from it on, by terminal cells."""
    used = 0
    for index, width in enumerate(cells(line)):
        if used >= column:
            return line[:index], line[index:]
        used += width
    return line, ""


def state_of(record, prompt_end):
    """One of empty, typing, other or alt, for a change record."""
    if record["alt"]:
        return "alt"
    if record["cursor"]["hidden"]:
        return "other"
    left, right = split_at_cursor(record["line"], record["cursor"]["col"])
    left, right = DECORATION.sub("", left), DECORATION.sub("", right)
    if left and prompt_end.search(left):
        return "empty" if not right else "typing"
    # "$ ls" has its cursor after "ls": a prompt glyph, then text.
    glyph = prompt_end.pattern.removesuffix("$")
    return "typing" if left and re.search(glyph + r"\s*\S", left) else "other"


def files_of(path):
    """The file given and the rotated files of the same pane beside it, oldest first."""
    path = Path(path)
    name = path.name.removesuffix(".jsonl")
    rotated = re.fullmatch(r"(.+)\.(\d+)", name)
    pane = rotated.group(1) if rotated else name
    older = sorted(
        (sibling for sibling in path.parent.glob(f"{pane}.*.jsonl") if ROTATED.match(sibling.name)),
        key=lambda sibling: int(ROTATED.match(sibling.name).group("number")),
    )
    live = path.with_name(f"{pane}.jsonl")
    return older + ([live] if live.exists() else [])


def read_records(path):
    """Every record of the pane in time order. A line that is not a record (the last one of a
    recording cut short) is skipped."""
    records = []
    for file in files_of(path):
        with open(file, encoding="utf-8") as lines:
            for line in lines:
                try:
                    records.append(json.loads(line))
                except json.JSONDecodeError:
                    continue
    records.sort(key=lambda record: (record["t"], record["m"], record.get("seq", 0)))
    seen, ordered = set(), []
    for record in records:
        key = json.dumps(record, sort_keys=True)
        if key not in seen:
            seen.add(key)
            ordered.append(record)
    return ordered


def screen_before(records, index):
    """The whole screen recorded last before `records[index]`, if any."""
    for record in reversed(records[:index]):
        if record["kind"] == "screen":
            return record
    return None


def transitions(records, prompt_end):
    """The changes of the input line to or from empty, each with how long the screen was still."""
    changes = [(index, record) for index, record in enumerate(records) if record["kind"] == "change"]
    found, previous = [], "other"
    for position, (index, record) in enumerate(changes):
        state = state_of(record, prompt_end)
        meaning = MEANING.get((previous, state))
        if meaning:
            following = changes[position + 1][1]["t"] if position + 1 < len(changes) else None
            found.append(
                {
                    "index": index,
                    "t": record["t"],
                    "seq": record["seq"],
                    "from": previous,
                    "to": state,
                    "meaning": meaning,
                    "line": record["line"],
                    "cursor": record["cursor"],
                    "above": record["above"],
                    "below": record["below"],
                    "still": None if following is None else (following - record["t"]) / 1000,
                }
            )
        previous = state
    return found


def clock(milliseconds):
    return datetime.fromtimestamp(milliseconds / 1000, timezone.utc).strftime("%H:%M:%S.%f")[:-3]


def show(found, records, first, with_screens):
    for item in found:
        still = "never changed again" if item["still"] is None else f"{item['still']:.2f}s"
        print(
            f"{clock(item['t'])} +{(item['t'] - first) / 1000:8.2f}s  #{item['seq']:<5} "
            f"{item['from']:>6} -> {item['to']:<6} {item['meaning']:<16} still {still}"
        )
        for row in item["above"]:
            print(f"      | {row}")
        print(f"      > {item['line']}    (cursor row {item['cursor']['row']}, col {item['cursor']['col']})")
        for row in item["below"]:
            print(f"      | {row}")
        if with_screens:
            screen = screen_before(records, item["index"])
            if screen:
                print(f"      whole screen as of {clock(screen['t'])}:")
                for number, row in enumerate(screen["rows"]):
                    print(f"      {number:2} {row}")
        print()


def summary(found, thresholds):
    print("transitions:")
    kinds = {}
    for item in found:
        kinds[item["meaning"]] = kinds.get(item["meaning"], 0) + 1
    for meaning, count in sorted(kinds.items()):
        print(f"  {meaning:<16} {count}")
    # Each time the line became empty, how long it then stayed still, and whether typing followed.
    empty = [item for item in found if item["to"] == "empty"]
    print(f"the line became empty {len(empty)} times")
    for seconds in thresholds:
        still = [item for item in empty if item["still"] is None or item["still"] >= seconds]
        print(f"  still for at least {seconds:g}s: {len(still)} of {len(empty)}")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("recording", help="a <pane>.jsonl file written by the screen recorder")
    parser.add_argument("--screens", action="store_true", help="print the whole screen at each moment")
    parser.add_argument("--json", action="store_true", help="print the transitions as JSON lines")
    parser.add_argument("--prompt-end", default=PROMPT_END, help="regex of the glyph that ends a prompt")
    parser.add_argument("--thresholds", default="1,2,5,10", help="seconds of stillness to count")
    arguments = parser.parse_args()
    records = read_records(arguments.recording)
    if not records:
        sys.exit("no records found")
    prompt_end = re.compile(arguments.prompt_end)
    found = transitions(records, prompt_end)
    if arguments.json:
        for item in found:
            print(json.dumps(item, ensure_ascii=False))
        return
    show(found, records, records[0]["t"], arguments.screens)
    summary(found, [float(value) for value in arguments.thresholds.split(",") if value])


if __name__ == "__main__":
    main()
