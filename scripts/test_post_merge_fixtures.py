"""Fixtures of the post_merge tests: a synthetic repository whose `scripts/check_all.sh` is a stand-in
for the gate, and a clone of it that stands in for the remote host."""

import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import gate_base
import gate_worktree
import post_merge
import post_merge_editor
import post_merge_hosts


# A stand-in for check_all.sh. The step named by FAKE_STEP (default `unit`) fails once the
# file `broken` exists; `tcc` also fails without its prerequisite `snap` in `--only`.
# FAKE_FLAKY=1 makes every `--only` run pass; FAKE_INFRA=1 makes the gate unable to start;
# FAKE_TRACE=FILE records each run's directory and arguments; FAKE_EVENTS=DIR keeps the
# `--event-file` of each run as DIR/<HEAD sha>.json. `--list` names a step per area:
# `eng` (engine) runs on the remote host, `unit`, `sw` and `tcc` run here.
FAKE_GATE = """#!/bin/bash
# supports --only
only=""; base=""; list=0; event=""
while (($#)); do case "$1" in --only) only="$2"; shift 2;; --base) base="$2"; shift 2;; --event-file) event="$2"; shift 2;; --list) list=1; shift;; *) shift;; esac; done
if ((list)); then printf 'always both unit\nengine both eng\nswift both sw\nswift full tcc\ne2e full snap\n'; exit 0; fi
timings="${SOYEHT_GATE_TIMINGS:?}"
step="${FAKE_STEP:-unit}"
[[ -z "${FAKE_TRACE:-}" ]] || echo "$PWD only=$only base=$base" >> "$FAKE_TRACE"
[[ -z "${FAKE_EVENTS:-}" || -z "$event" ]] || cp "$event" "$FAKE_EVENTS/$(git rev-parse HEAD).json"
[[ -z "${FAKE_INFRA:-}" ]] || exit 71
status=0
if [[ -n "$only" ]]; then
    known=0; for id in unit eng sw tcc snap; do [[ ",$only," == *",$id,"* ]] && known=1; done
    ((known)) || exit 78
    [[ ",$only," == *",$step,"* ]] || exit 0
    [[ -z "${FAKE_FLAKY:-}" ]] || exit 0
    [[ "$step" != tcc || ",$only," == *",snap,"* ]] || exit 1
fi
[[ -e broken ]] && status=1
printf '{"latest":{"steps":[{"id":"%s","area":"always","seconds":1.0,"status":%s}]}}' "$step" "$status" > "$timings"
((status)) && echo "check_all: FAILED step $step (area always, exit 1)" >&2
exit "$status"
"""


# The parts the stand-ins with a list of steps share: reading `--only` and `--list`, and recording
# a step in the timings file the way check_all.sh does, stopping at the first failure.
FAKE_ONLY_ARGUMENTS = r"""only=""; list=0
while (($#)); do case "$1" in --only) only="$2"; shift 2;; --list) list=1; shift;; *) shift;; esac; done
"""
FAKE_RECORD_STEP = r"""    steps+="${steps:+,}{\"id\":\"$id\",\"area\":\"always\",\"seconds\":1.0,\"status\":$status}"
    printf '{"latest":{"steps":[%s]}}' "$steps" > "${SOYEHT_GATE_TIMINGS:?}"
    if ((status)); then echo "check_all: FAILED step $id (area always, exit $status)" >&2; exit "$status"; fi
done
"""

# A stand-in with an editor measurement step, run in order after `unit` and before `ts`. The step
# reads `.cache/editor-performance-vm.json` like the real check: no file fails as a missing
# report, `stale` in it (or a version other than the commit's `editor_version`) as editor files that changed, `slow` as a latency over the limit.
# `broken` makes the step named by FAKE_BREAK (default `unit`) fail; the file FAKE_ONCE makes it fail
# once (the failure removes the file), as a flake does.
FAKE_EDITOR_GATE = """#!/bin/bash
# supports --only
""" + FAKE_ONLY_ARGUMENTS + """if ((list)); then printf 'always both unit\neditor_perf full editor-performance:check\ntypescript both ts\n'; exit 0; fi
[[ -z "${FAKE_TRACE:-}" ]] || echo "$PWD only=$only" >> "$FAKE_TRACE"
steps=""
editor() {
    local report=.cache/editor-performance-vm.json
    [[ -f "$report" ]] || { echo "editor-performance: missing $PWD/$report; run Release UI measurement on VM1" >&2; return 1; }
    # An `editor_version` file in the commit stands for its editor files: the report must name it.
    if [[ -f editor_version ]] && ! grep -q "$(cat editor_version)" "$report"; then
        echo "editor-performance: editor paths changed; measure this editor version again on VM1" >&2; return 1
    fi
    if grep -q stale "$report"; then
        echo "editor-performance: editor paths changed; measure this editor version again on VM1" >&2; return 1
    fi
    if grep -q slow "$report"; then
        echo "editor-performance: key_work_p95_ms: median 9.000 ms >= 8.0 ms; profile synchronous editor work" >&2
        return 1
    fi
}
for id in unit editor-performance:check ts; do
    [[ -z "$only" || ",$only," == *",$id,"* ]] || continue
    status=0
    if [[ "$id" == editor-performance:check ]]; then editor || status=$?
    elif [[ -e broken && "$id" == "${FAKE_BREAK:-unit}" ]]; then status=1
    elif [[ -n "${FAKE_ONCE:-}" && -e "$FAKE_ONCE" && "$id" == "${FAKE_BREAK:-unit}" ]]; then rm -f "$FAKE_ONCE"; status=1; fi
""" + FAKE_RECORD_STEP


def git(root: Path, *arguments: str) -> str:
    return subprocess.run(
        ["git", "-c", "user.name=t", "-c", "user.email=t@example.com", *arguments],
        cwd=root, check=True, capture_output=True, text=True,
    ).stdout.strip()


def commit(root: Path, name: str, files: dict[str, str | None]) -> str:
    for path, content in files.items():
        target = root / path
        if content is None:
            target.unlink()
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content)
    git(root, "add", "-A")
    git(root, "commit", "-q", "-m", name)
    return git(root, "rev-parse", "HEAD")


def isolate_environment():
    """Inside a `mock.patch.dict(os.environ)`: forget the variables post_merge reads, which a
    watch wrapper may export (they would send the tests to a real worktree, meter or host)."""
    for name in (post_merge.WORKTREE_ENV, post_merge_editor.EDITOR_METER_ENV, post_merge_hosts.SSH_ENV,
                 gate_base.MAIN_HEALTH_DIR_ENV):
        os.environ.pop(name, None)


class SyntheticRepositoryCase(unittest.TestCase):
    FILES = {"scripts/check_all.sh": FAKE_GATE, "a": "1"}

    def setUp(self):
        self.root = Path(tempfile.mkdtemp()) / "repo"
        self.root.mkdir()
        git(self.root, "init", "-q", "-b", "main")
        self.health = self.root.parent / "health"
        self.gate = "bash scripts/check_all.sh".split()
        self.shas = [commit(self.root, "start", self.FILES)]
        self.scratch = self.root.parent / "scratch"
        self.scratch.mkdir()
        environment = mock.patch.dict(
            os.environ,
            {"FORBIDDEN_TERMS": "placeholder", gate_worktree.SCRATCH_ROOT_ENV: str(self.scratch)},
        )
        environment.start()
        self.addCleanup(environment.stop)
        isolate_environment()
        self.addCleanup(shutil.rmtree, self.root.parent, ignore_errors=True)

    def add(self, name, files):
        self.shas.append(commit(self.root, name, files))
        return self.shas[-1]

    def run_main(self, sha, *extra):
        return post_merge.main(
            ["--repo", str(self.root), "--sha", sha, "--health-dir", str(self.health), *extra]
        )

    def record(self, sha):
        return json.loads((self.health / f"{sha}.json").read_text())


FAKE_SSH = """#!/bin/bash
# Runs the remote command here, as the host would; the first argument is the host name.
[[ -z "${FAKE_SSH_LOG:-}" ]] || echo "$1" >> "$FAKE_SSH_LOG"
shift
exec bash -c "$*"
"""


class RemoteHostCase(SyntheticRepositoryCase):
    """A repository whose clone stands in for the Linux host, reached through a fake ssh."""

    FILES = {
        **SyntheticRepositoryCase.FILES,
        "scripts/remote_gate.sh": (Path(__file__).with_name("remote_gate.sh")).read_text(),
    }

    def setUp(self):
        super().setUp()
        self.host_checkout = self.root.parent / "host"
        subprocess.run(["git", "clone", "-q", str(self.root), str(self.host_checkout)], check=True)
        self.fake_ssh = self.root.parent / "fake_ssh"
        self.fake_ssh.write_text(FAKE_SSH)
        self.fake_ssh.chmod(0o755)
        self.trace = self.root.parent / "trace.txt"
        self.ssh_log = self.root.parent / "ssh.txt"
        environment = mock.patch.dict(os.environ, {
            post_merge_hosts.SSH_ENV: str(self.fake_ssh), "FAKE_TRACE": str(self.trace),
            "FAKE_SSH_LOG": str(self.ssh_log),
        })
        environment.start()
        self.addCleanup(environment.stop)

    def run_remote(self, sha, *extra, root=None):
        return self.run_main(sha, "--remote", "linux-host", "--remote-root",
                             str(root or self.host_checkout), *extra)

    def runs(self):
        return [line.split(" ", 1) for line in self.trace.read_text().splitlines()]
