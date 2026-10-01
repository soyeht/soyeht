#!/usr/bin/env python3
"""Fail when acceptance runs make macOS privacy (TCC) ask about their executables.

`snapshot FILE` records the time and this checkout's TCC records before the
runs; `check FILE` fails if a record was added or changed since then, or if the
unified log shows a privacy request made on behalf of one of the executables
the acceptances run (their supervisor, engine and CLI, see `e2e.programs_directory`).
Both commands only read the user's TCC database and log. When either source is
unreadable (for example without Full Disk Access) that source is skipped with a
message instead of failing the gate.
"""

import argparse
import json
import re
import sqlite3
import subprocess
import sys
import time
from contextlib import closing
from datetime import datetime
from pathlib import Path

from e2e import programs_directory

TCC_DATABASE = Path.home() / "Library/Application Support/com.apple.TCC/TCC.db"
# tccd logs every request at the default level, which the log keeps for days:
# AUTHREQ_CTX names the service and AUTHREQ_SUBJECT the executable, joined by
# the daemon and message id. A request whose answer is already stored (for
# example a silent denial) still logs both, although the database is unchanged.
LOG_PREDICATE = ('subsystem == "com.apple.TCC" AND (eventMessage BEGINSWITH "AUTHREQ_CTX: "'
                 ' OR eventMessage BEGINSWITH "AUTHREQ_SUBJECT: ")')
FIELD = re.compile(r"(\w+)=([^,]*)")
# Every exec asks whether its parent is a developer tool; it never prompts.
IGNORED_SERVICES = frozenset({"kTCCServiceDeveloperTool"})


def read_records(database, prefix):
    """Map (service, client) to last_modified for path clients under `prefix`."""
    uri = f"{database.as_uri()}?mode=ro"
    query = ("select service, client, last_modified from access "
             "where client_type = 1 and substr(client, 1, ?) = ?")
    with closing(sqlite3.connect(uri, uri=True)) as connection:
        rows = connection.execute(query, (len(prefix), prefix)).fetchall()
    return {f"{service}\t{client}": modified for service, client, modified in rows}


def changed_records(before, after):
    return sorted(key for key, modified in after.items() if before.get(key) != modified)


def requests_in_log(lines, prefix):
    """Privacy requests, other than ignored services, made for an executable under `prefix`."""
    services, subjects = {}, {}
    for line in lines:
        try:
            event = json.loads(line)
        except ValueError:
            continue
        message = event.get("eventMessage", "")
        kind, _, rest = message.partition(": ")
        fields = dict(FIELD.findall(rest))
        key = (event.get("processID"), fields.get("msgID"))
        if kind == "AUTHREQ_CTX" and "service" in fields:
            services[key] = fields["service"]
        elif kind == "AUTHREQ_SUBJECT" and "subject" in fields:
            subjects[key] = fields["subject"]
    return sorted({f"{services[key]} for {subject}" for key, subject in subjects.items()
                   if subject.startswith(prefix) and key in services
                   and services[key] not in IGNORED_SERVICES})


def prefixes():
    directory = programs_directory()
    return sorted({f"{directory}/", f"{directory.resolve()}/"})


def current_records():
    records = {}
    for prefix in prefixes():
        records.update(read_records(TCC_DATABASE, prefix))
    return records


def snapshot(path):
    state = {"started": time.time(), "records": None}
    try:
        state["records"] = current_records()
    except sqlite3.Error as error:
        print(f"tcc_guard: TCC database unreadable ({error}); record check skipped")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(state))


def log_lines(started):
    start = datetime.fromtimestamp(started - 1).strftime("%Y-%m-%d %H:%M:%S")
    result = subprocess.run(
        ["/usr/bin/log", "show", "--style", "ndjson", "--start", start,
         "--predicate", LOG_PREDICATE],
        capture_output=True, text=True, timeout=600,
    )
    if result.returncode != 0:
        raise OSError(result.stderr.strip() or "log show failed")
    return result.stdout.splitlines()


def check(path):
    state = json.loads(path.read_text())
    problems = []
    if state["records"] is not None:
        try:
            problems += [f"TCC record added or changed: {key}"
                         for key in changed_records(state["records"], current_records())]
        except sqlite3.Error as error:
            print(f"tcc_guard: TCC database unreadable ({error}); record check skipped")
    try:
        lines = log_lines(state["started"])
        for prefix in prefixes():
            problems += [f"privacy request: {message}" for message in requests_in_log(lines, prefix)]
    except (OSError, subprocess.SubprocessError) as error:
        print(f"tcc_guard: unified log unreadable ({error}); request check skipped")
    for problem in problems:
        print(f"tcc_guard: {problem}", file=sys.stderr)
    if problems:
        print("tcc_guard: acceptance runs must not ask for macOS privacy access", file=sys.stderr)
        return 1
    print("tcc_guard: no privacy requests from acceptance executables")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("snapshot", "check"))
    parser.add_argument("state", type=Path)
    args = parser.parse_args()
    if args.command == "snapshot":
        snapshot(args.state)
        return 0
    return check(args.state)


if __name__ == "__main__":
    raise SystemExit(main())
