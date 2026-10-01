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


JSCPD_VERSION = "4.3.0"
MACHETE_VERSION = "0.9.2"
# Generous ceilings: a killed scanner is a false failure on a busy machine.
SCAN_TIMEOUT = 300
TOOL_TIMEOUT = 60
FILE_LIMIT = 800
AREAS = {
    "engine": ("engine", "rust", ".rs"),
    "shared": ("apps/shared", "swift", ".swift"),
    "mac": ("apps/mac/Sources", "swift", ".swift"),
    "ios": ("apps/ios/Sources", "swift", ".swift"),
}
DUPLICATION_SCANS = {
    **{
        name: ((relative_dir,), language)
        for name, (relative_dir, language, _) in AREAS.items()
    },
    "swift_all": (("apps/shared", "apps/mac/Sources", "apps/ios/Sources"), "swift"),
}
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


def duplicate_measurement(repo: Path, area: str, jscpd: Path) -> dict[str, int | float]:
    paths, language = DUPLICATION_SCANS[area]
    with tempfile.TemporaryDirectory(prefix="soyeht-quality-jscpd-") as output:
        command = [
            str(jscpd),
            "--min-lines",
            "5",
            "--min-tokens",
            "40",
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
        try:
            result = subprocess.run(
                command, cwd=repo, capture_output=True, text=True, timeout=SCAN_TIMEOUT
            )
        except subprocess.TimeoutExpired as error:
            raise QualityError(
                f"jscpd exceeded {SCAN_TIMEOUT} s for {area}; the machine is overloaded"
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


def source_measurements(repo: Path) -> tuple[dict[str, int], dict[str, dict[str, int]]]:
    oversized: dict[str, int] = {}
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


def machete_status(repo: Path, executable: str) -> str:
    result = subprocess.run(
        [executable, "--skip-target-dir", str(repo / "engine")],
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
        "version": 2,
        "duplication": duplicates,
        "oversized_files": oversized,
        "loose_literals": literals,
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
    }


def differences(baseline: dict, actual: dict) -> list[tuple[str, str]]:
    """Return (direction, explanation); only RELAXATION entries fail the gate."""
    changes: list[tuple[str, str]] = []
    for area in DUPLICATION_SCANS:
        for metric in ("clones", "duplicated_lines"):
            current = actual["duplication"][area][metric]
            previous = baseline["duplication"][area][metric]
            if current != previous:
                direction = "RELAXATION" if current > previous else "IMPROVEMENT"
                changes.append(
                    (
                        direction,
                        f"duplication {area}.{metric}: baseline {previous}, actual {current}",
                    )
                )
    previous_files = baseline["oversized_files"]
    current_files = actual["oversized_files"]
    for path in sorted(previous_files.keys() | current_files.keys()):
        previous = previous_files.get(path, FILE_LIMIT)
        current = current_files.get(path, FILE_LIMIT)
        if current != previous:
            direction = "RELAXATION" if current > previous else "IMPROVEMENT"
            changes.append(
                (
                    direction,
                    f"file {path}: cap {previous}, actual {current}"
                    + ("; split the file" if direction == "RELAXATION" else ""),
                )
            )
    previous_literals = baseline["loose_literals"]
    current_literals = actual["loose_literals"]
    for path in sorted(previous_literals.keys() | current_literals.keys()):
        previous_counts = previous_literals.get(path, {})
        current_counts = current_literals.get(path, {})
        for kind in sorted(previous_counts.keys() | current_counts.keys()):
            previous = previous_counts.get(kind, 0)
            current = current_counts.get(kind, 0)
            if current != previous:
                direction = "RELAXATION" if current > previous else "IMPROVEMENT"
                changes.append(
                    (
                        direction,
                        f"literal {path} {kind}: baseline {previous}, actual {current}"
                        + (
                            "; use a policy, token, font source or limits"
                            if direction == "RELAXATION"
                            else ""
                        ),
                    )
                )
    return changes


def baseline_relaxations(before: dict, after: dict) -> list[str]:
    """Highlight every loosened baseline entry, including exception caps."""
    return [
        message
        for direction, message in differences(before, after)
        if direction == "RELAXATION"
    ]
