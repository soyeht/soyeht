"""Shared measurements and comparisons for the local quality gate."""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import NamedTuple

from check_swallowed_errors import measure as swallowed_errors
from hardening_gate import engine_production_panics as count_engine_panics


JSCPD_VERSION = "4.3.0"
MACHETE_VERSION = "0.9.2"
# Generous ceilings: a killed scanner is a false failure on a busy machine.
# The scan deadline starts at SCAN_TIMEOUT and grows with the load per core up to
# SCAN_TIMEOUT_CEILING, so a loaded host is given time and a real hang still fails.
# SOYEHT_SCAN_TIMEOUT (seconds) replaces the computed value.
SCAN_TIMEOUT = 300
SCAN_TIMEOUT_CEILING = 1800
SCAN_TIMEOUT_ENV = "SOYEHT_SCAN_TIMEOUT"
TOOL_TIMEOUT = 60
FILE_LIMIT = 800
# The Python of the gate scripts and the contract generator: the same file limit as the other
# languages (tests included), and one duplication scan.
PYTHON_DIRS = ("scripts", "contracts")
GENERATED_PYTHON_HEADER = "# Generated from contracts/"
AREAS = {
    "engine": ("engine", "rust", ".rs"),
    "desktop": ("apps/desktop", "rust", ".rs"),
    "shared": ("apps/shared", "swift", ".swift"),
    "dictation": ("apps/dictation", "swift", ".swift"),
    "mac": ("apps/mac/Sources", "swift", ".swift"),
    "ios": ("apps/ios/Sources", "swift", ".swift"),
}
DUPLICATION_SCANS = {
    **{
        name: ((relative_dir,), language)
        for name, (relative_dir, language, _) in AREAS.items()
    },
    "swift_all": (
        ("apps/shared", "apps/dictation", "apps/mac/Sources", "apps/ios/Sources"),
        "swift",
    ),
    # The desktop views are built from chained calls that the 40-token scan never sees as
    # repeated: a smaller window finds the widgets written twice.
    "desktop_views": (("apps/desktop/crates/soyeht-desktop/src",), "rust"),
    # The gate scripts, the end-to-end harness and the contract generator are code too: the
    # e2e files repeat their setup, and nothing else counts it.
    "python": (PYTHON_DIRS, "python"),
}
# (minimum lines, minimum tokens) of a clone; the scans not listed use the default.
DEFAULT_CLONE_SIZE = (5, 40)
CLONE_SIZES = {"desktop_views": (4, 25)}
SWIFT_LITERAL_PATTERNS = {
    "padding": re.compile(r"\.padding\(\s*(?:\.[A-Za-z]+\s*,\s*)?\d+(?:\.\d+)?\s*\)"),
    "corner_radius": re.compile(
        r"(?:cornerRadius\s*:\s*|\.cornerRadius\(\s*)\d+(?:\.\d+)?"
    ),
    "system_font_size": re.compile(r"\.system\(\s*size:\s*\d+(?:\.\d+)?"),
    "font_name": re.compile(
        r'"(?:JetBrains Mono|Nunito|Courier Prime|Newsreader)(?:-[A-Za-z]+| [A-Za-z]+)?"'
        r'|(?:\.custom|NSFont|UIFont|font\(named:)\s*\(?(?:name:\s*)?"[^"\n]+"'
    ),
}
RUST_LITERAL_PATTERN = re.compile(r"\b(?:4096|512)\b")
IGNORED_PARTS = {"target", ".build", ".cache", "node_modules"}
IGNORED_FILES = {"generated.rs", "Generated.swift"}
BASELINE_NAME = "scripts/quality_baseline.json"
JUSTIFICATIONS_NAME = "scripts/quality_baseline_justifications.json"
PANIC_KEY = "engine_production_panics"
# try?, .ok() and let _ = in production code: each new one must log, propagate,
# surface the error, or be justified like any other baseline raise.
SWALLOWED_KEY = "swallowed_errors"
# Absolute: production code returns errors; no baseline or reason can raise it.
PANIC_CEILING = 0
MEASUREMENT_VERSION = 3


class Change(NamedTuple):
    """One baseline entry that differs; only RELAXATION means "got worse"."""

    direction: str
    key: str
    current: int
    message: str


class QualityError(Exception):
    """A gate prerequisite or measurement failed."""


def install_jscpd_offline(repo: Path) -> str:
    """Reproduce the locked jscpd from the local npm cache; never use the network."""
    tools = repo / "scripts/quality"
    if not (tools / "package-lock.json").is_file() or shutil.which("npm") is None:
        return "npm or scripts/quality/package-lock.json is unavailable"
    try:
        result = subprocess.run(
            ["npm", "ci", "--ignore-scripts", "--offline", "--no-audit", "--no-fund"],
            cwd=tools,
            capture_output=True,
            text=True,
            timeout=TOOL_TIMEOUT,
        )
    except subprocess.TimeoutExpired:
        return "offline npm ci timed out"
    if result.returncode:
        return "the local npm cache does not hold the locked packages"
    return ""


def find_machete() -> str | None:
    found = shutil.which("cargo-machete")
    if found:
        return found
    cargo_home = Path(os.environ.get("CARGO_HOME") or Path.home() / ".cargo")
    candidate = cargo_home / "bin/cargo-machete"
    return str(candidate) if candidate.is_file() else None


def tool_version(executable: str, prefix: str = "") -> str:
    result = subprocess.run(
        [executable, "--version"],
        capture_output=True,
        text=True,
        timeout=TOOL_TIMEOUT,
    )
    output = result.stdout.strip()
    return output.removeprefix(prefix).strip() if result.returncode == 0 else ""


def tool_paths(repo: Path) -> tuple[Path, str]:
    """Locate the pinned tools; a missing jscpd is restored offline from the npm cache."""
    jscpd = repo / "scripts/quality/node_modules/.bin/jscpd"
    if not jscpd.is_file():
        problem = install_jscpd_offline(repo)
        if problem or not jscpd.is_file():
            raise QualityError(
                f"jscpd {JSCPD_VERSION} is missing and could not be restored offline "
                f"({problem or 'npm ci produced no jscpd'}); run once with network: "
                "npm ci --prefix scripts/quality --ignore-scripts "
                "(the gate itself never downloads packages)"
            )
    if tool_version(str(jscpd)) != JSCPD_VERSION:
        raise QualityError(
            f"jscpd must be {JSCPD_VERSION}; run: "
            "npm ci --prefix scripts/quality --ignore-scripts"
        )
    machete = find_machete()
    install = f"cargo install cargo-machete --version {MACHETE_VERSION} --locked"
    if machete is None:
        raise QualityError(f"cargo-machete is missing; run: {install}")
    if tool_version(machete, "cargo-machete") != MACHETE_VERSION:
        raise QualityError(f"cargo-machete must be {MACHETE_VERSION}; run: {install}")
    return jscpd, machete


def source_files(repo: Path, relative_dir: str, suffix: str) -> list[Path]:
    directory = repo / relative_dir
    return sorted(
        path
        for path in directory.rglob(f"*{suffix}")
        if path.is_file()
        and not IGNORED_PARTS.intersection(path.relative_to(repo).parts)
        and path.name not in IGNORED_FILES
    )


def rust_roots(repo: Path) -> list[Path]:
    """The Rust areas present in `repo`; a base from before an area existed lacks it."""
    return [
        repo / relative_dir
        for relative_dir, language, _ in AREAS.values()
        if language == "rust" and (repo / relative_dir).is_dir()
    ]


def duplicate_measurement(repo: Path, area: str, jscpd: Path) -> dict[str, int | float]:
    paths, language = DUPLICATION_SCANS[area]
    min_lines, min_tokens = CLONE_SIZES.get(area, DEFAULT_CLONE_SIZE)
    # A scan may span several folders; one a checkout does not have yet is not an error.
    paths = tuple(path for path in paths if (repo / path).is_dir())
    if not paths:
        return {"clones": 0, "duplicated_lines": 0, "percent": 0}
    with tempfile.TemporaryDirectory(prefix="soyeht-quality-jscpd-") as output:
        command = [
            str(jscpd),
            "--min-lines",
            str(min_lines),
            "--min-tokens",
            str(min_tokens),
            "--max-lines",
            "20000",
            "--max-size",
            "2mb",
            "--format",
            language,
            "--reporters",
            "json",
            "--output",
            output,
            "--ignore",
            "**/generated.rs,**/Generated.swift,**/target/**,**/.build/**",
            "--silent",
            "--noTips",
            *paths,
        ]
        deadline = scan_timeout()
        try:
            result = subprocess.run(
                command, cwd=repo, capture_output=True, text=True, timeout=deadline
            )
        except subprocess.TimeoutExpired as error:
            raise QualityError(
                f"jscpd exceeded {deadline:.0f} s for {area}; the machine is overloaded "
                f"or the scan hung (set {SCAN_TIMEOUT_ENV} to allow more time)"
            ) from error
        report = Path(output) / "jscpd-report.json"
        if result.returncode or not report.is_file():
            detail = (result.stderr or result.stdout).strip()
            raise QualityError(f"jscpd failed for {area}: {detail[:500]}")
        total = json.loads(report.read_text())["statistics"]["total"]
    if total["sources"] == 0:
        raise QualityError(f"jscpd found no sources in {area}; check the scan paths")
    return {
        "clones": total["clones"],
        "duplicated_lines": total["duplicatedLines"],
        "percent": total["percentage"],
    }


def scan_timeout(
    load: float | None = None, cores: int | None = None, environment: dict[str, str] | None = None
) -> float:
    """Return the jscpd deadline in seconds for the current host load."""
    environment = os.environ if environment is None else environment
    configured = environment.get(SCAN_TIMEOUT_ENV)
    if configured:
        try:
            seconds = float(configured)
        except ValueError:
            seconds = 0.0
        if seconds <= 0:
            raise QualityError(f"{SCAN_TIMEOUT_ENV} must be a positive number of seconds")
        return seconds
    if load is None:
        try:
            load = os.getloadavg()[0]
        except OSError:
            load = 0.0  # No load average on this system: keep the base deadline.
    cores = cores or os.cpu_count() or 1
    return min(SCAN_TIMEOUT_CEILING, SCAN_TIMEOUT * max(1.0, load / cores))


def oversized_python(repo: Path) -> dict[str, int]:
    """Python files over FILE_LIMIT lines, a generated output excepted (it is not written by hand)."""
    oversized: dict[str, int] = {}
    for relative_dir in PYTHON_DIRS:
        for path in source_files(repo, relative_dir, ".py"):
            lines = path.read_text(encoding="utf-8").splitlines()
            if len(lines) > FILE_LIMIT and not (
                lines and lines[0].startswith(GENERATED_PYTHON_HEADER)
            ):
                oversized[path.relative_to(repo).as_posix()] = len(lines)
    return oversized


def source_measurements(repo: Path) -> tuple[dict[str, int], dict[str, dict[str, int]]]:
    oversized = oversized_python(repo)
    literals: dict[str, dict[str, int]] = {}
    for relative_dir, _, suffix in AREAS.values():
        for path in source_files(repo, relative_dir, suffix):
            relative_path = path.relative_to(repo).as_posix()
            lines = path.read_text(encoding="utf-8").splitlines()
            if len(lines) > FILE_LIMIT:
                oversized[relative_path] = len(lines)
            if path.name == "limits.rs":
                continue
            counts = dict.fromkeys(
                SWIFT_LITERAL_PATTERNS if suffix == ".swift" else ("rust_limits",),
                0,
            )
            for line in lines:
                if suffix == ".swift":
                    for name, pattern in SWIFT_LITERAL_PATTERNS.items():
                        counts[name] += len(pattern.findall(line))
                else:
                    counts["rust_limits"] += len(RUST_LITERAL_PATTERN.findall(line))
            nonzero = {name: count for name, count in counts.items() if count}
            if nonzero:
                literals[relative_path] = nonzero
    return oversized, literals


def engine_production_panics(repo: Path) -> int:
    """unwrap/expect/panic! in engine and desktop production code; the ceiling is 0."""
    return count_engine_panics(
        {
            path.relative_to(repo).as_posix(): path.read_text(encoding="utf-8")
            for root in rust_roots(repo)
            for path in source_files(repo, root.relative_to(repo).as_posix(), ".rs")
        }
    )


def machete_status(repo: Path, executable: str) -> str:
    result = subprocess.run(
        [
            executable,
            "--skip-target-dir",
            *map(str, rust_roots(repo)),
        ],
        cwd=repo,
        capture_output=True,
        text=True,
        timeout=TOOL_TIMEOUT,
    )
    if result.returncode == 0:
        return "PASS"
    detail = (result.stdout + result.stderr).strip()
    if result.returncode == 1:
        raise QualityError(
            "cargo-machete found unused dependencies; remove them or document a "
            f"false positive in Cargo.toml. {detail[:600]}"
        )
    raise QualityError(f"cargo-machete could not scan the workspace: {detail[:600]}")


def periphery_status(repo: Path) -> str:
    executable = shutil.which("periphery")
    if executable is None:
        return "SKIPPED (Periphery is not installed)"
    index = repo / ".cache/mac-derived/Index.noindex/DataStore"
    if not index.is_dir():
        return "SKIPPED (no Mac index store; build the Mac app first)"
    command = [
        executable,
        "scan",
        "--project",
        "apps/mac/SoyehtMac.xcodeproj",
        "--schemes",
        "SoyehtMac",
        "--skip-build",
        "--index-store-path",
        str(index),
        "--disable-update-check",
        "--format",
        "json",
        "--quiet",
    ]
    try:
        result = subprocess.run(
            command, cwd=repo, capture_output=True, text=True, timeout=20
        )
    except subprocess.TimeoutExpired:
        return "SKIPPED (Periphery scan exceeded the 20-second hook budget)"
    if result.returncode:
        return "SKIPPED (Periphery scan failed; inspect its local configuration)"
    try:
        findings = json.loads(result.stdout)
    except ValueError:
        return "SKIPPED (Periphery did not emit JSON)"
    if not isinstance(findings, list):
        return "SKIPPED (unexpected Periphery report format)"
    return f"OBSERVED ({len(findings)} possible unused Swift declarations; advisory)"


def measure(repo: Path, jscpd: Path, machete: str) -> dict:
    oversized, literals = source_measurements(repo)
    # The scans are independent processes; run them together to keep the hook fast.
    with ThreadPoolExecutor(max_workers=len(DUPLICATION_SCANS) + 1) as pool:
        unused_dependencies = pool.submit(machete_status, repo, machete)
        scans = {
            area: pool.submit(duplicate_measurement, repo, area, jscpd)
            for area in DUPLICATION_SCANS
        }
        duplicates = {area: scan.result() for area, scan in scans.items()}
        unused_dependencies.result()
    return {
        "version": MEASUREMENT_VERSION,
        "duplication": duplicates,
        "oversized_files": oversized,
        "loose_literals": literals,
        PANIC_KEY: engine_production_panics(repo),
        SWALLOWED_KEY: swallowed_errors(repo),
    }


def baseline_from_measurement(measurement: dict) -> dict:
    """Persist only gated values; percentages remain report-only information."""
    return {
        "version": measurement["version"],
        "duplication": {
            area: {
                metric: measurement["duplication"][area][metric]
                for metric in ("clones", "duplicated_lines")
            }
            for area in DUPLICATION_SCANS
        },
        "oversized_files": measurement["oversized_files"],
        "loose_literals": measurement["loose_literals"],
        PANIC_KEY: measurement[PANIC_KEY],
        SWALLOWED_KEY: measurement[SWALLOWED_KEY],
    }


def compare(key: str, previous: int, current: int, label: str, hint: str = "") -> list[Change]:
    if current == previous:
        return []
    direction = "RELAXATION" if current > previous else "IMPROVEMENT"
    suffix = hint if direction == "RELAXATION" else ""
    return [Change(direction, key, current, label + suffix)]


def differences(baseline: dict, actual: dict) -> list[Change]:
    """Every entry where `actual` differs from `baseline`, in a stable order."""
    changes: list[Change] = []
    for area in DUPLICATION_SCANS:
        for metric in ("clones", "duplicated_lines"):
            current = actual["duplication"][area][metric]
            # An area added after the older baseline was written had nothing to hold yet.
            previous = baseline["duplication"].get(area, {}).get(metric, 0)
            changes += compare(
                f"duplication.{area}.{metric}",
                previous,
                current,
                f"duplication {area}.{metric}: baseline {previous}, actual {current}",
            )
    previous_files = baseline["oversized_files"]
    current_files = actual["oversized_files"]
    for path in sorted(previous_files.keys() | current_files.keys()):
        previous = previous_files.get(path, FILE_LIMIT)
        current = current_files.get(path, FILE_LIMIT)
        changes += compare(
            f"oversized_files.{path}",
            previous,
            current,
            f"file {path}: cap {previous}, actual {current}",
            "; split the file",
        )
    previous_literals = baseline["loose_literals"]
    current_literals = actual["loose_literals"]
    for path in sorted(previous_literals.keys() | current_literals.keys()):
        previous_counts = previous_literals.get(path, {})
        current_counts = current_literals.get(path, {})
        for kind in sorted(previous_counts.keys() | current_counts.keys()):
            previous = previous_counts.get(kind, 0)
            current = current_counts.get(kind, 0)
            changes += compare(
                f"loose_literals.{path}.{kind}",
                previous,
                current,
                f"literal {path} {kind}: baseline {previous}, actual {current}",
                "; use a policy, token, font source or limits",
            )
    # A side recorded before this metric existed has nothing to compare.
    if SWALLOWED_KEY in baseline and SWALLOWED_KEY in actual:
        for metric in sorted(baseline[SWALLOWED_KEY].keys() | actual[SWALLOWED_KEY].keys()):
            previous = baseline[SWALLOWED_KEY].get(metric, 0)
            current = actual[SWALLOWED_KEY].get(metric, 0)
            changes += compare(
                f"{SWALLOWED_KEY}.{metric}",
                previous,
                current,
                f"swallowed errors {metric}: baseline {previous}, actual {current}",
                "; log, propagate or surface the error, or justify the ignore",
            )
    # Absolute ceiling, whatever the older side recorded.
    if PANIC_KEY in actual and actual[PANIC_KEY] > PANIC_CEILING:
        changes += compare(
            PANIC_KEY,
            PANIC_CEILING,
            actual[PANIC_KEY],
            f"engine production unwrap/expect/panic!: allowed {PANIC_CEILING}, "
            f"actual {actual[PANIC_KEY]}",
            "; return an error instead",
        )
    return changes


def baseline_relaxations(before: dict, after: dict) -> list[Change]:
    """Every loosened baseline entry, including exception caps."""
    # A duplication area the older baseline did not scan yet has nothing to compare.
    before = {**before, "duplication": {**after["duplication"], **before["duplication"]}}
    # A swallowed-error metric the older baseline did not count yet has nothing to compare.
    before[SWALLOWED_KEY] = {**after[SWALLOWED_KEY], **before.get(SWALLOWED_KEY, {})}
    return [
        change for change in differences(before, after) if change.direction == "RELAXATION"
    ]


def baseline_at(repo: Path, commit: str) -> dict | None:
    """The checked-in baseline at `commit`, or None when it did not exist yet."""
    result = subprocess.run(
        ["git", "show", f"{commit}:{BASELINE_NAME}"],
        cwd=repo,
        capture_output=True,
        text=True,
    )
    return json.loads(result.stdout) if result.returncode == 0 else None


def load_justifications(repo: Path) -> dict[str, dict]:
    """Map of baseline key to {"value": raised value, "reason": why}."""
    path = repo / JUSTIFICATIONS_NAME
    if not path.is_file():
        return {}
    data = json.loads(path.read_text())
    if not isinstance(data, dict):
        raise QualityError(f"{JUSTIFICATIONS_NAME} must hold one JSON object")
    return data


def justification_for(change: Change, justifications: dict[str, dict]) -> str:
    """The written reason for raising this entry to its new value, or ""."""
    if change.key == PANIC_KEY:
        return ""  # absolute; no reason can raise it
    entry = justifications.get(change.key)
    if not isinstance(entry, dict) or entry.get("value") != change.current:
        return ""
    reason = entry.get("reason")
    return reason.strip() if isinstance(reason, str) else ""


def unjustified_relaxations(
    previous: dict | None, baseline: dict, justifications: dict[str, dict]
) -> list[Change]:
    """Baseline entries raised since `previous` without a matching written reason."""
    if previous is None:
        return []
    return [
        change
        for change in baseline_relaxations(previous, baseline)
        if not justification_for(change, justifications)
    ]
