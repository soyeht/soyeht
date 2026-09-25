#!/usr/bin/env python3
"""Release editor UI measurement on VM1, and report validation in check_all.

Run --measure in a VM1 checkout and copy .cache/editor-performance-vm.json to
the review checkout. The ordinary gate never launches a UI test on the host.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / ".cache"
VM_REPORT = CACHE / "editor-performance-vm.json"
REPORT = CACHE / "editor-performance.json"
RUST_REPORT = CACHE / "editor-performance-rust.json"
WORK_LIMIT_MS = 8.0
METER_BUNDLE_ID = "com.soyeht.next.dev.mac.editor-meter"
RUST_LIMITS = {"open_100k_ms": 150.0, "search_1gb_ms": 1000.0}
RUN_COUNT = 3


def editor_path(path: str) -> bool:
    """Only editor, client, protocol, and meter files invalidate the VM result."""
    return (
        path.startswith("apps/mac/Sources/Editor") and path.endswith(".swift")
        or path == "apps/mac/Sources/SoyehtMacApp.swift"
        or path.startswith("apps/mac/PerformanceUITests/")
        or path in {
            "apps/mac/SoyehtMac.xcodeproj/project.pbxproj",
            "apps/mac/SoyehtMac.xcodeproj/xcshareddata/xcschemes/SoyehtMac.xcscheme",
            "contracts/protocol.json",
            "scripts/check_editor_performance.py",
        }
        or path.startswith("apps/shared/Sources/SoyehtClient/")
        or path.startswith("contracts/swift/Sources/SoyehtProtocol/")
        or path.startswith("engine/soyeht-protocol/")
    )


def editor_tree_hash(root: Path = ROOT) -> str:
    tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=root)
    paths = {path.decode() for path in tracked.split(b"\0") if path}
    # Uncommitted new fixture files must influence the first measurement too.
    for directory in (root / "apps/mac/PerformanceUITests", root / "apps/mac/Sources"):
        if directory.is_dir():
            paths.update(str(path.relative_to(root)) for path in directory.rglob("*.swift"))
    digest = hashlib.sha256()
    for path in sorted(filter(editor_path, paths)):
        digest.update(path.encode() + b"\0")
        file = root / path
        digest.update(file.read_bytes() if file.is_file() else b"<deleted>")
        digest.update(b"\0")
    return digest.hexdigest()


def head(root: Path = ROOT) -> str:
    return subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()


def report_failures(report: dict, expected_hash: str) -> list[str]:
    failures: list[str] = []
    if report.get("configuration") != "Release":
        failures.append("configuration must be Release; measure again on VM1")
    if report.get("editor_tree_hash") != expected_hash:
        failures.append("editor paths changed; measure this editor version again on VM1")
    runs = report.get("runs")
    if not isinstance(runs, list) or len(runs) != RUN_COUNT:
        return failures + [f"expected {RUN_COUNT} complete VM runs"]
    for index, run in enumerate(runs, 1):
        for key, minimum in (
            ("key_event_samples", 240),
            ("validated_key_samples", 240),
            ("key_work_samples", 240),
            ("scroll_events", 48),
            ("scroll_work_samples", 20),
            ("page_load_samples", 20),
        ):
            value = run.get(key)
            if not isinstance(value, int) or value < minimum:
                failures.append(f"run {index}: {key} < {minimum}; rerun VM UI test")
        if run.get("visual_validation") != "real_window_screenshot_and_gutter":
            failures.append(f"run {index}: visible ink/gutter proof missing")
        if run.get("file_line_count") != 100_000:
            failures.append(f"run {index}: wrong large-file fixture")
        for key in ("key_work_p95_ms", "scroll_work_p95_ms", "page_load_p95_ms"):
            value = run.get(key)
            if not isinstance(value, (int, float)) or not 0 <= value < 10_000:
                failures.append(f"run {index}: invalid {key}")
    for key in ("key_work_p95_ms", "scroll_work_p95_ms"):
        values = [run.get(key) for run in runs]
        if all(isinstance(value, (int, float)) for value in values):
            median = statistics.median(values)
            if median >= WORK_LIMIT_MS:
                failures.append(
                    f"{key}: median {median:.3f} ms >= {WORK_LIMIT_MS:.1f} ms; "
                    "profile synchronous editor work")
    return failures


def run_vm_measurement() -> dict:
    """Run only in VM1; UI automation must not commandeer the host desktop."""
    if os.environ.get("SOYEHT_EDITOR_METER_VM") != "1":
        raise RuntimeError("set SOYEHT_EDITOR_METER_VM=1 only inside VM1")
    team = os.environ.get("SOYEHT_DEVELOPMENT_TEAM")
    if not team:
        raise RuntimeError("set SOYEHT_DEVELOPMENT_TEAM locally on VM1")
    CACHE.mkdir(exist_ok=True)
    derived = CACHE / "mac-derived-editor-meter"
    packages = CACHE / "apple-packages"
    command = [
        "xcodebuild", "-project", "apps/mac/SoyehtMac.xcodeproj", "-scheme", "SoyehtMac",
        "-configuration", "Release", "-destination", "platform=macOS,arch=arm64",
        "-derivedDataPath", str(derived), "-clonedSourcePackagesDirPath", str(packages),
        "-skipPackagePluginValidation", "-parallel-testing-enabled", "NO",
        "ONLY_ACTIVE_ARCH=YES", "SWIFT_ACTIVE_COMPILATION_CONDITIONS=SOYEHT_PERFORMANCE_METER",
        "CODE_SIGN_STYLE=Automatic", f"SOYEHT_APP_BUNDLE_ID={METER_BUNDLE_ID}",
    ]
    environment = os.environ.copy()
    environment["SOYEHT_PACKAGES"] = str(packages)
    environment["DEVELOPMENT_TEAM"] = team
    environment["TEST_RUNNER_SOYEHT_EDITOR_METER_APP_BUNDLE_ID"] = METER_BUNDLE_ID
    runs = []
    # Both the app and runner can access this owner-only neutral path; it is
    # outside either app's container and is removed even when a run fails.
    with tempfile.TemporaryDirectory(prefix="soyeht-editor-meter-", dir="/private/tmp") as scratch:
        scratch_path = Path(scratch)
        for index in range(RUN_COUNT):
            raw = scratch_path / f"run-{index + 1}.json"
            bundle = scratch_path / f"run-{index + 1}.xcresult"
            environment["TEST_RUNNER_SOYEHT_EDITOR_PERFORMANCE_REPORT"] = str(raw)
            log = CACHE / f"editor-performance-run-{index + 1}.log"
            result = subprocess.run(
                command + [
                    "-resultBundlePath", str(bundle),
                    "-only-testing:EditorPerformanceUITests/EditorPerformanceUITests/testEditorWorkAndVisiblePages",
                    "test",
                ],
                cwd=ROOT, env=environment, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                check=False, text=True, errors="replace",
            )
            sanitized = (result.stdout or "").replace(team, "[redacted]")
            sanitized = re.sub(
                r"Apple Development: [^\r\n\"]+", "Apple Development: [redacted]",
                sanitized,
            )
            log.write_text(sanitized)
            if result.returncode:
                evidence = Path(tempfile.mkdtemp(prefix="editor-performance-failure-", dir=CACHE))
                for suffix in (".error", ".partial"):
                    artifact = raw.with_suffix(raw.suffix + suffix)
                    if artifact.is_file():
                        shutil.copy2(artifact, evidence / artifact.name)
                if bundle.is_dir():
                    shutil.copytree(bundle, evidence / bundle.name)
                raise RuntimeError(
                    f"VM UI test run {index + 1} failed; inspect {log} and {evidence}")
            if not raw.is_file():
                raise RuntimeError(f"VM UI test run {index + 1} produced no app report")
            run = json.loads(raw.read_text())
            # xcodebuild passed the UI assertion of >=20 real-window screenshots.
            run["visual_validation"] = "real_window_screenshot_and_gutter"
            runs.append(run)
    report = {
        "configuration": "Release",
        "measured_head": head(),
        "editor_tree_hash": editor_tree_hash(),
        "runs": runs,
        "known_issue": "#404: page-load work is diagnostic until the virtual editor lands",
    }
    VM_REPORT.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    failures = report_failures(report, editor_tree_hash())
    if failures:
        raise RuntimeError("; ".join(failures))
    return report


def check() -> dict:
    if not VM_REPORT.is_file():
        raise RuntimeError(f"missing {VM_REPORT}; run Release UI measurement on VM1")
    vm_report = json.loads(VM_REPORT.read_text())
    failures = report_failures(vm_report, editor_tree_hash())
    if failures:
        raise RuntimeError("; ".join(failures))
    subprocess.run(
        [
            "cargo", "run", "--locked", "--release", "--quiet", "-p", "soyeht-editor",
            "--example", "editor_performance", "--", "--report", str(RUST_REPORT),
        ],
        cwd=ROOT / "engine", check=True,
    )
    rust = json.loads(RUST_REPORT.read_text())
    for key, limit in RUST_LIMITS.items():
        if float(rust[key]) >= limit:
            raise RuntimeError(f"{key}={rust[key]} ms >= {limit} ms")
    report = vm_report | {"rust": rust, "checked_head": head()}
    REPORT.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--measure", action="store_true", help="run signed Release UI test on VM1")
    options = parser.parse_args()
    try:
        report = run_vm_measurement() if options.measure else check()
        for key, count_key in (
            ("key_work_p95_ms", "key_work_samples"),
            ("scroll_work_p95_ms", "scroll_work_samples"),
            ("page_load_p95_ms", "page_load_samples"),
        ):
            values = [run[key] for run in report["runs"]]
            counts = [run[count_key] for run in report["runs"]]
            print(
                f"editor-performance: {key} n={counts} runs={values} "
                f"median={statistics.median(values):.3f}")
        print("editor-performance: page load diagnostic #404; passed")
        return 0
    except (
        OSError, RuntimeError, KeyError, TypeError, ValueError,
        json.JSONDecodeError, subprocess.CalledProcessError,
    ) as error:
        print(f"editor-performance: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
