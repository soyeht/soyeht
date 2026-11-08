#!/usr/bin/env python3
"""Hold down the request parameters the engine still reads by key from untyped JSON.

The engine receives the parameters of a request as a `Value`, which `validate::parameters`
checks at the edge. A handler that reads `params["cols"].as_u64()` names the field by hand and
decides again what a missing or mistyped value means, so an optional field that becomes required
(or the reverse) compiles and fails at run time. The generated `...Params` type of the method
cannot drift: a handler deserializes it once and reads typed fields. The number of `params[...]`
reads in the production code of `soyeht-engine` may only go down.

The allowed count is `params_reads` in scripts/params_reads_baseline.json. When the count
falls, lower the baseline in the same change:

    uv run python scripts/check_params_reads.py --print-baseline > scripts/params_reads_baseline.json
"""

from __future__ import annotations

import re
from pathlib import Path

from check_event_literals import production_sources
from count_ratchet import run

BASELINE = Path(__file__).resolve().parent / "params_reads_baseline.json"
KEY = "params_reads"
# `params[` after a path (`request.params[`) counts too; a longer name (`other_params[`) does not.
PARAMS_READ = re.compile(r"(?<![A-Za-z0-9_])params\s*\[")


def sites(root: Path) -> list[tuple[str, int]]:
    """Return (path, line) for every `params[...]` read in production code."""
    return [
        (path.relative_to(root).as_posix(), source.count("\n", 0, match.start()) + 1)
        for path, source, production in production_sources(root)
        for match in PARAMS_READ.finditer(production)
    ]


def main() -> int:
    return run(
        description=__doc__,
        label="params reads",
        key=KEY,
        baseline=BASELINE,
        find=sites,
        site="params[...]",
        noun="untyped params reads",
        fix="deserialize the method's generated Params type instead",
        baseline_name=BASELINE.name,
    )


if __name__ == "__main__":
    raise SystemExit(main())
