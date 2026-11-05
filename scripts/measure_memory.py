#!/usr/bin/env python3
"""Measure the memory of the engine and the supervisor under a repeatable terminal load.

Starts a disposable supervisor and engine (plain child processes, never the service manager) from
the programs of `--bin-dir`, drives shell panes through the real socket and records the resident
size of both after each phase:

  idle      the engine and the supervisor with no pane
  panes     `--panes` idle shells
  output    `--heavy` of them print `--lines` lines each (`seq`), no client attached
  viewer    a client replays the tail of every pane, as a window that opens would
  cycles    every pane is closed and new ones print again, `--cycles` times: growth is a leak
  closed    every pane closed
  restart   the engine restarted with the closed panes still in its database
  editor    `--files` text files of `--file-megabytes` MB each opened in editor panes, then closed
  outage    the engine is down while `--outage` shells print `--outage-megabytes` MB each (the
            supervisor holds it), then the engine comes back and takes it in

Run it twice, on the base and on the change, with the same arguments, and compare the reports:

    cargo build --release -p soyeht-cli -p soyeht-engine -p soyeht-ptyd -p soyeht-mcp
    uv run python scripts/measure_memory.py --report /tmp/after.json

The numbers are the process's resident set (`ps`) and, on macOS, its physical footprint
(`footprint`), which counts compressed pages. Close other heavy work first: the numbers move
with allocator reuse, not with load, but the output phase is paced by the shell.
"""
import argparse
import base64
import json
import os
import re
import subprocess
import time
from pathlib import Path

os.environ.setdefault("SOYEHT_E2E_LAUNCHER", "process")

from e2e import ROOT, PRODUCT_NAMES, Services, require_binaries  # noqa: E402
from e2e_platform import MACOS, require  # noqa: E402

FOOTPRINT = re.compile(r"phys_footprint:\s*([\d.]+)\s*(\w+)")
UNITS = {"B": 1, "KB": 1024, "MB": 1024 ** 2, "GB": 1024 ** 3}
SETTLE_SECONDS = 1.0


def resident_mb(pid):
    """The resident set of `pid` in MB, from `ps`."""
    output = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True)
    return round(int(output.stdout.strip() or 0) / 1024, 1)


def footprint_mb(pid):
    """The physical footprint of `pid` in MB (macOS only), or None."""
    if not MACOS:
        return None
    output = subprocess.run(["footprint", "-p", str(pid)], capture_output=True, text=True)
    match = FOOTPRINT.search(output.stdout)
    if match is None:
        return None
    return round(float(match.group(1)) * UNITS[match.group(2).upper()] / 1024 ** 2, 1)


def sample(services):
    """The memory of the engine and the supervisor, in MB."""
    result = {}
    for component in ("engine", "ptyd"):
        pid = services.launcher.processes[component].pid
        result[component] = {"rss": resident_mb(pid), "footprint": footprint_mb(pid)}
    return result


def line_bytes(lines):
    """Bytes `seq 1 lines` prints."""
    return sum(len(str(number)) + 1 for number in range(1, lines + 1))


def stream_offsets(client):
    return {s["paneId"]: int(s["nextOffset"]) for s in client.snapshot()[1]["streams"]}


def wait_quiet(client, pane_ids, minimum):
    """Waits until every pane has produced at least `minimum` bytes and then stops producing."""
    deadline = time.monotonic() + 300
    last, stable_since = None, time.monotonic()
    while True:
        offsets = stream_offsets(client)
        now = {pane: offsets.get(pane, 0) for pane in pane_ids}
        if now != last:
            last, stable_since = now, time.monotonic()
        elif time.monotonic() - stable_since >= SETTLE_SECONDS and all(
                value >= minimum for value in now.values()):
            return
        require(time.monotonic() < deadline, "the panes did not finish printing")
        time.sleep(0.2)


def create_panes(services, client, count):
    panes = []
    for index in range(count):
        created = services.cli("pane", "create", "--shell", "--idempotency-key",
                               f"measure-{time.monotonic_ns()}-{index}")
        panes.append(created["paneId"])
    deadline = time.monotonic() + 60
    while any(client.pane(pane_id) is None or not client.pane(pane_id).get("pid")
              for pane_id in panes):
        require(time.monotonic() < deadline, "the shells did not start")
        time.sleep(0.2)
    return panes


def print_lines(client, pane_ids, lines):
    command = base64.b64encode(f"seq 1 {lines}\n".encode()).decode()
    for pane_id in pane_ids:
        client.mutate("pane.input", paneId=pane_id, bytes=command)
    wait_quiet(client, pane_ids, line_bytes(lines))


def close_panes(client, pane_ids):
    for pane_id in pane_ids:
        client.mutate("pane.close", paneId=pane_id)


def outage(services, client, args):
    """Stops the engine while shells print more than the supervisor keeps; returns the client of
    the engine that took the output in, and the supervisor's memory with the output held."""
    panes = create_panes(services, client, args.outage)
    command = base64.b64encode(
        f"sleep 1; head -c {args.outage_megabytes * 1024 * 1024} /dev/zero | tr '\\0' x\n".encode()).decode()
    expected = {pane_id: stream_offsets(client).get(pane_id, 0) for pane_id in panes}
    for pane_id in panes:
        client.mutate("pane.input", paneId=pane_id, bytes=command)
    client.close()
    services.stop_engine()
    time.sleep(args.outage_megabytes + 4)
    held = sample(services)
    services.start("engine")
    client = services.client("measure")
    wait_quiet(client, panes, min(expected.values()) + args.outage_megabytes * 1024 * 1024)
    return client, panes, held


def text_file(path, megabytes):
    line = ("The quick brown fox jumps over the lazy dog; " * 2).rstrip() + "\n"
    with open(path, "w") as output:
        for _ in range(megabytes * 1024 * 1024 // len(line)):
            output.write(line)


def editor(services, client, args):
    """Opens large files in editor panes and closes them again; returns the memory with them open."""
    workspace = client.snapshot()[1]["workspaces"][0]["workspaceId"]
    folder = services.data / "files"
    folder.mkdir(exist_ok=True)
    panes = []
    for index in range(args.files):
        path = folder / f"large-{index}.txt"
        text_file(path, args.file_megabytes)
        opened = client.mutate("file.open", workspaceId=workspace, path=str(path))
        panes.append(opened["pane"]["paneId"])
    time.sleep(SETTLE_SECONDS)
    opened = sample(services)
    close_panes(client, panes)
    time.sleep(SETTLE_SECONDS)
    return opened


def run(services, args):
    report = {"arguments": {k: v for k, v in vars(args).items() if k not in ("report", "bin_dir")},
              "phases": {}}
    phases = report["phases"]
    client = services.client("measure")
    time.sleep(SETTLE_SECONDS)
    phases["idle"] = sample(services)

    panes = create_panes(services, client, args.panes)
    time.sleep(SETTLE_SECONDS)
    phases["panes"] = sample(services)

    heavy = panes[:args.heavy]
    print_lines(client, heavy, args.lines)
    phases["output"] = sample(services)

    for pane_id in panes:
        client.replay(pane_id)
    time.sleep(SETTLE_SECONDS)
    phases["viewer"] = sample(services)

    close_panes(client, panes)
    for cycle in range(args.cycles):
        panes = create_panes(services, client, args.panes)
        print_lines(client, panes[:args.heavy], args.lines)
        for pane_id in panes:
            client.replay(pane_id)
        close_panes(client, panes)
    time.sleep(SETTLE_SECONDS)
    phases["cycles"] = sample(services)

    time.sleep(SETTLE_SECONDS)
    phases["closed"] = sample(services)

    client = services.restart_engine()
    time.sleep(SETTLE_SECONDS)
    phases["restart"] = sample(services)

    if args.files:
        phases["editor open"] = editor(services, client, args)
        phases["editor closed"] = sample(services)

    if args.outage:
        client, panes, phases["outage"] = outage(services, client, args)
        time.sleep(SETTLE_SECONDS)
        phases["outage taken in"] = sample(services)
        close_panes(client, panes)
        time.sleep(SETTLE_SECONDS)
        phases["outage closed"] = sample(services)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--bin-dir", type=Path, default=ROOT / "engine/target/release")
    parser.add_argument("--panes", type=int, default=12)
    parser.add_argument("--heavy", type=int, default=4)
    parser.add_argument("--lines", type=int, default=200000)
    parser.add_argument("--cycles", type=int, default=3)
    parser.add_argument("--files", type=int, default=3,
                        help="large files opened in the editor (0 skips the phase)")
    parser.add_argument("--file-megabytes", type=int, default=20)
    parser.add_argument("--outage", type=int, default=2,
                        help="shells that print while the engine is down (0 skips the phase)")
    parser.add_argument("--outage-megabytes", type=int, default=12)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    binaries = args.bin_dir.resolve()
    require_binaries(binaries, PRODUCT_NAMES, "build the release programs first (see --help)")
    services = Services(binaries)
    try:
        services.start("ptyd")
        services.start("engine")
        report = run(services, args)
    finally:
        services.close()
    text = json.dumps(report, indent=2)
    print(text)
    if args.report:
        args.report.write_text(text + "\n")


if __name__ == "__main__":
    main()
