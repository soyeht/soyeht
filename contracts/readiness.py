"""Renders the `x-readiness` section of protocol.json for the engine.

The tools the engine runs (`git`, `wt`, ...) and the harness CLIs a person should have, with the
command that installs each on every system the checklist knows. The engine answers
`engine.readiness` from this table, so the clients show one hint and never keep a copy.
"""

import json
import subprocess

RUST_OUTPUT = "engine/soyeht-protocol/src/readiness.rs"


def validate(section, schema):
    systems = schema["$defs"]["ReadinessSystem"]["enum"]
    known = [system for system in systems if system != "other"]
    if set(section) != {"tools", "harnesses", "notes"}:
        raise ValueError("x-readiness needs exactly tools, harnesses and notes")
    if any(set(note) != {"prefix", "text"} or not all(note.values()) for note in section["notes"]):
        raise ValueError("a note of x-readiness is a prefix and a text")
    seen = set()
    for entry in section["tools"] + section["harnesses"]:
        if entry["id"] in seen:
            raise ValueError(f"x-readiness lists {entry['id']} twice")
        seen.add(entry["id"])
        tool = "name" in entry
        applies = entry["systems"] if tool else known
        if not applies or not set(applies) <= set(known):
            raise ValueError(f"{entry['id']} applies to {known} only")
        if not set(entry["hints"]) <= set(applies) or not all(entry["hints"].values()):
            raise ValueError(f"{entry['id']} hints must be commands for a system it applies to")
        if tool and type(entry["required"]) is not bool:
            raise ValueError(f"{entry['id']} says whether it is required")
    if any(not entry["id"].startswith("registry.") for entry in section["harnesses"]):
        raise ValueError("a harness of x-readiness is a registry entry")


def variant(system):
    return system[:1].upper() + system[1:]


def hints_rust(hints):
    pairs = ", ".join(f"(ReadinessSystem::{variant(s)}, {json.dumps(h)})" for s, h in hints.items())
    return f"Hints(&[{pairs}])"


def systems_rust(systems, known):
    if systems == known:
        return "EVERYWHERE"
    return f"&[{', '.join('ReadinessSystem::' + variant(s) for s in systems)}]"


def render_rust(section, header, known):
    tools = "\n".join(
        f"    Tool {{ id: {json.dumps(t['id'])}, name: {json.dumps(t['name'])}, "
        f"required: {str(t['required']).lower()}, "
        f"systems: {systems_rust(t['systems'], known)}, "
        f"hints: {hints_rust(t['hints'])} }},"
        for t in section["tools"]
    )
    harnesses = "\n".join(
        f"    Harness {{ id: {json.dumps(h['id'])}, hints: {hints_rust(h['hints'])} }},"
        for h in section["harnesses"]
    )
    notes = "\n".join(
        f"    Note {{ prefix: {json.dumps(n['prefix'])}, text: {json.dumps(n['text'])} }},"
        for n in section["notes"]
    )
    source = f"""{header}use crate::ReadinessSystem;

/// The commands that install one program, by system.
pub struct Hints(&'static [(ReadinessSystem, &'static str)]);

impl Hints {{
    /// The command that installs it on `system`, when there is one.
    pub fn for_system(&self, system: &ReadinessSystem) -> Option<&'static str> {{
        self.0
            .iter()
            .find(|(candidate, _)| candidate == system)
            .map(|(_, command)| *command)
    }}
}}

/// What to say with a command that starts with `prefix`: what it needs first.
pub struct Note {{
    pub prefix: &'static str,
    pub text: &'static str,
}}

/// The note that goes with an install `hint`, if its command has a prerequisite.
pub fn note_for(hint: &str) -> Option<&'static str> {{
    NOTES
        .iter()
        .find(|note| hint.starts_with(note.prefix))
        .map(|note| note.text)
}}

/// A program the engine runs.
pub struct Tool {{
    pub id: &'static str,
    pub name: &'static str,
    /// Without it part of the engine does not work, not only a convenience.
    pub required: bool,
    /// The systems the engine runs it on.
    pub systems: &'static [ReadinessSystem],
    pub hints: Hints,
}}

/// The CLI of a harness of the registry.
pub struct Harness {{
    pub id: &'static str,
    pub hints: Hints,
}}

/// The systems the checklist knows commands for.
const EVERYWHERE: &[ReadinessSystem] = &[{', '.join('ReadinessSystem::' + variant(s) for s in known)}];

pub const NOTES: &[Note] = &[
{notes}
];

pub const TOOLS: &[Tool] = &[
{tools}
];

pub const HARNESSES: &[Harness] = &[
{harnesses}
];
"""
    return subprocess.run(
        ["rustfmt", "--edition", "2024"], input=source, text=True, capture_output=True, check=True
    ).stdout


def render_readiness(schema, header):
    """The generated files for protocol.json's `x-readiness`, under the shared `header`."""
    section = schema["x-readiness"]
    validate(section, schema)
    known = [system for system in schema["$defs"]["ReadinessSystem"]["enum"] if system != "other"]
    return {RUST_OUTPUT: render_rust(section, header, known)}
