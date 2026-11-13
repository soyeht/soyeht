"""The wire contract only grows (ADR 0161).

A client in the field speaks an older version of `contracts/protocol.json` than the engine it
meets, so a change that an old client cannot read is a break. `problems` compares two contracts and
names each break; `released_problems` runs it against the snapshots of the last releases kept in
`contracts/released/` (one `protocol-v<N>.json` per protocol version, written by
`scripts/release.py snapshot`), so `generate.py --check` fails on a change that is not additive.
A break is made on purpose by raising `x-protocolVersion.min` with a `minReason`, which also means
deleting the snapshots below the new `min`.

The direction of a type decides what may change. A type reachable from the params of a method is
read by the engine (up), one reachable from a result or an event is read by the client (down), and
one that is both, or neither, gets the stricter rule of the two.
"""

import json
from pathlib import Path

RELEASED = "contracts/released"
SCHEMA = "contracts/protocol.json"
# The parts of the contract that are the wire: what a client and the engine say to each other.
SURFACE = ("$defs", "x-methods", "x-events", "x-errorCodes", "x-framing", "x-limits")
# The bounds of a value, and whether a lower number is the looser one.
BOUNDS = {"minimum": True, "minLength": True, "maximum": False, "maxLength": False}


def _without_descriptions(node):
    if isinstance(node, dict):
        return {key: _without_descriptions(value) for key, value in node.items() if key != "description"}
    if isinstance(node, list):
        return [_without_descriptions(value) for value in node]
    return node


def surface(schema):
    """The wire of a contract, with the prose left out (a changed description is not a change)."""
    return _without_descriptions({key: schema.get(key, {}) for key in SURFACE}) | {
        "x-openEnums": sorted(schema.get("x-openEnums", []))
    }


def _references(node, found):
    if isinstance(node, dict):
        for key, value in node.items():
            if key == "$ref":
                found.add(value.rsplit("/", 1)[-1])
            else:
                _references(value, found)
    elif isinstance(node, list):
        for value in node:
            _references(value, found)
    return found


def _reachable(defs, roots):
    seen, pending = set(), list(roots)
    while pending:
        name = pending.pop()
        if name not in seen and name in defs:
            seen.add(name)
            pending.extend(_references(defs[name], set()))
    return seen


def directions(wire):
    """The names of the types the engine reads (up) and the ones the client reads (down)."""
    defs = wire["$defs"]
    up, down = set(), set()
    for method in wire["x-methods"].values():
        up |= _references(method.get("params", {}), set())
        down |= _references(method.get("result", {}), set())
    for event in wire["x-events"].values():
        down |= _references(event.get("params", {}), set())
    down.add("RpcError")
    return _reachable(defs, up), _reachable(defs, down)


class _Rules:
    """Compares one type of the old contract with the same type of the new one."""

    def __init__(self, name, up, down, opened):
        self.name = name
        self.only_up = up and not down
        self.only_down = down and not up
        # Only a type the engine reads (or one declared open) takes a new enum value; a type
        # nobody is known to read, or both sides read, gets the stricter rule.
        self.grows = self.only_up or opened
        self.found = []

    def problem(self, path, text):
        self.found.append(f"{self.name}{path}: {text}")

    def compare(self, old, new, path=""):
        if "$ref" in old or "$ref" in new:
            if old.get("$ref") != new.get("$ref"):
                self.problem(path, f"the type changed from {old.get('$ref')} to {new.get('$ref')}")
            return
        if old.get("type") != new.get("type"):
            self.problem(path, f"the type changed from {old.get('type')} to {new.get('type')}")
            return
        for key in ("format", "pattern", "contentEncoding"):
            if old.get(key) != new.get(key):
                self.problem(path, f"{key} changed")
        self.bounds(old, new, path)
        self.choices("enum", old, new, path)
        if "oneOf" in old or "oneOf" in new:
            self.alternatives(old.get("oneOf", []), new.get("oneOf", []), path)
        if "items" in old:
            self.compare(old["items"], new.get("items", {}), path + "[]")
        if old.get("additionalProperties") is True and new.get("additionalProperties") is not True:
            self.problem(path, "unknown properties are no longer ignored")
        self.properties(old, new, path)

    def bounds(self, old, new, path):
        for key, lower_is_looser in BOUNDS.items():
            before, after = old.get(key), new.get(key)
            if before == after:
                continue
            # A missing bound is the loosest; the reader of a value may accept more of it and
            # the writer may send less, and a type both sides use keeps its bounds.
            infinity = float("-inf") if lower_is_looser else float("inf")
            wide = [infinity if bound is None else bound for bound in (before, after)]
            looser = wide[1] < wide[0] if lower_is_looser else wide[1] > wide[0]
            if (looser and self.only_up) or (not looser and self.only_down):
                continue
            self.problem(path, f"{key} changed from {before} to {after}")

    def choices(self, key, old, new, path):
        if key not in old and key not in new:
            return
        before, after = old.get(key, []), new.get(key, [])
        for value in before:
            if value not in after:
                self.problem(path, f"the {key} value {value!r} was removed")
        added = [value for value in after if value not in before]
        if added and not self.grows:
            self.problem(path, f"the {key} value(s) {added} were added to a type the engine sends")

    def alternatives(self, old, new, path):
        if len(new) < len(old):
            self.problem(path, "an alternative was removed")
        for index, (before, after) in enumerate(zip(old, new)):
            self.compare(before, after, f"{path}<{index}>")
        if len(new) > len(old) and not self.grows:
            self.problem(path, "an alternative was added to a type the engine sends")

    def properties(self, old, new, path):
        before, after = old.get("properties", {}), new.get("properties", {})
        required_before, required_after = set(old.get("required", [])), set(new.get("required", []))
        for name, node in before.items():
            if name not in after:
                self.problem(f"{path}.{name}", "the property was removed")
                continue
            self.compare(node, after[name], f"{path}.{name}")
            if (name in required_before) != (name in required_after):
                self.problem(f"{path}.{name}", "the property changed between required and optional")
        for name in after.keys() - before.keys():
            if name in required_after and not self.only_down:
                self.problem(f"{path}.{name}", "a new property that is required, in a type the engine reads")


def problems(old, new):
    """What an old client cannot read in `new`, one line each; both are `surface` or contracts."""
    old, new = surface(old), surface(new)
    found = []
    for kind in ("x-methods", "x-events"):
        for name, entry in old[kind].items():
            if name not in new[kind]:
                found.append(f"{kind[2:-1]} {name}: removed")
                continue
            for key in ("params", "result", "durable"):
                if entry.get(key) != new[kind][name].get(key):
                    found.append(f"{kind[2:-1]} {name}: {key} changed")
    for kind, code in old["x-errorCodes"].items():
        if new["x-errorCodes"].get(kind) != code:
            found.append(f"error code {kind}: removed or renumbered")
    for name, value in old["x-limits"].items():
        if name not in new["x-limits"] or new["x-limits"][name] < value:
            found.append(f"limit {name}: removed or lowered")
    for name, value in old["x-framing"].items():
        if new["x-framing"].get(name) != value:
            found.append(f"framing {name}: changed")
    for name in old["x-openEnums"]:
        if name not in new["x-openEnums"]:
            found.append(f"{name}: the enum was open and is closed again")
    up, down = directions(new)
    for name, node in old["$defs"].items():
        if name not in new["$defs"]:
            found.append(f"{name}: the type was removed")
            continue
        rules = _Rules(name, name in up, name in down, name in new["x-openEnums"])
        rules.compare(node, new["$defs"][name])
        found += rules.found
    return found


def snapshot_path(root, number):
    return Path(root) / RELEASED / f"protocol-v{number}.json"


def snapshots(root):
    """The released surfaces by protocol version."""
    found = {}
    for path in sorted((Path(root) / RELEASED).glob("protocol-v*.json")):
        number = path.stem.removeprefix("protocol-v")
        found[int(number)] = json.loads(path.read_text())
    return found


def released_problems(root, schema):
    """What the contract breaks against the releases a client may be at; nothing before the first."""
    versions = schema["x-protocolVersion"]
    low, high = versions["min"], versions["current"]
    released = snapshots(root)
    if not released:
        return []
    found = []
    wire = surface(schema)
    for number, old in released.items():
        name = f"protocol-v{number}.json"
        if number < low:
            found.append(f"{RELEASED}/{name}: below min {low}, delete it (a break needs its minReason)")
            continue
        found += [f"against {name}: {line}" for line in problems(old, wire)]
    if max(released) > high:
        found.append(f"current is {high}, below the released version {max(released)}")
    if released.get(high, wire) != wire:
        found.append(f"the wire differs from protocol-v{high}.json: raise x-protocolVersion.current")
    return found


def snapshot_to_write(root, schema):
    """The file `write_snapshot` would create (its path), or None when the contract of this protocol
    version is already recorded. A record that differs is an error, as it is for the write."""
    number = schema["x-protocolVersion"]["current"]
    path = snapshot_path(root, number)
    if not path.exists():
        return path
    if json.loads(path.read_text()) != surface(schema):
        raise ValueError(f"{path.name} exists and differs: raise x-protocolVersion.current instead of replacing it")
    return None


def write_snapshot(root, schema, product_version):
    """Records the contract of the release being made; a version already recorded is never replaced."""
    path = snapshot_to_write(root, schema)
    if path is None:
        return None
    number = schema["x-protocolVersion"]["current"]
    wire = surface(schema)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(wire, indent=1, ensure_ascii=False) + "\n")
    index = Path(root) / RELEASED / "index.json"
    entries = json.loads(index.read_text()) if index.exists() else {}
    entries[str(number)] = {"productVersion": product_version}
    index.write_text(json.dumps(entries, indent=2, sort_keys=True) + "\n")
    return path


def missing_snapshot(root, schema):
    """The line a release is refused with when its protocol version has no snapshot (or another)."""
    number = schema["x-protocolVersion"]["current"]
    path = snapshot_path(root, number)
    if not path.exists():
        return f"{RELEASED}/{path.name} is missing: run `uv run python scripts/release.py snapshot`"
    if json.loads(path.read_text()) != surface(schema):
        return f"{RELEASED}/{path.name} differs from protocol.json: raise x-protocolVersion.current"
    return None
