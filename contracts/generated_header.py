"""The one header every generated output carries.

It names the source file and nothing else. A hash of the source would change with every edit to
the source, so two pull requests that edit different parts of the same contract would conflict in
every output. `generate.py --check` compares the whole regenerated content, so a stale output is
caught without it.
"""

# The prefix that marks an output. `scripts/check_wire_names.py` reads it from here to skip
# outputs, and the generator's tests check every output against it.
PREFIX = "Generated from contracts/"


def header_text(source):
    """The header text for an output produced from `source` (a path under contracts/)."""
    if not source.startswith("contracts/"):
        raise ValueError(f"{source} is not a contract file")
    return f"Generated from {source}. Do not edit."
