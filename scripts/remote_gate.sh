#!/bin/bash
# The remote side of scripts/post_merge.py --remote (docs/dev-local.md, ADR 0089).
#
#   remote_gate.sh --sha FULL_SHA --steps ID[,ID...] [--base FULL_SHA] [--slot NAME]
#   remote_gate.sh --print-timings [--slot NAME]
#
# Run from a checkout of the repository on a Linux host. It fetches the commit, checks it out
# in a persistent worktree of that checkout (`.cache/remote-gate/tree`, so the build
# directories survive between runs) and runs `check_all.sh --tier full --only STEPS` there.
# The step output goes to stdout and stderr and the exit code is the gate's. Exit 2 means the
# host could not run the gate (the commit cannot be fetched or checked out). The restricted
# vocabulary is never sent to the host: the steps that run here do not scan for it, and a
# placeholder satisfies the gate's check that it is set. `--print-timings` prints the
# per-step times of the last run.
#
# `--slot NAME` uses the worktree `.cache/remote-gate/tree-NAME` and its own timings file, so
# two commits can run at once on one host; a worktree takes one run at a time (exit 75 when it
# is busy, where `flock` exists). The default worktree is `tree`.
#
# On a Linux host with Nix (NixOS, where a rustup toolchain cannot run) the gate runs in the
# shell of scripts/linux-gate.nix, that of the commit when it has one, else the checkout's:
# the pinned toolchain, the GPUI libraries, the tools, and `soyeht-gate-fhs`, a root with the
# usual Linux paths. Its caches (cargo, sccache, uv, temporary files) live under the state
# directory. Every run is `nice`d (SOYEHT_GATE_NICE, default 15) and capped at
# SOYEHT_GATE_JOBS cargo jobs (default 32) and SOYEHT_GATE_TEST_THREADS test threads (16).
#
# Host settings (SOYEHT_GATE_TMPDIR, SOYEHT_GATE_GIT, SOYEHT_GATE_JOBS, ...) may come from the
# environment of the call or from the file `gate.env` in the state directory (SOYEHT_GATE_ENV_FILE
# names another): shell assignments, read before anything else, so a caller that cannot pass
# variables over ssh still configures the host. git is SOYEHT_GATE_GIT when set, else the first
# on PATH; a non-interactive ssh session may have none, and the Nix shell brings its own.
#
# After the gate's own output come these lines, which a caller may read:
#   remote_gate: SECONDS STATUS STEP_ID     one per step that ran
#   remote_gate: gate-timings PATH
#   remote_gate: exit CODE
set -euo pipefail

self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
root="$(cd "$(dirname "$0")/.." && pwd)"
state="${SOYEHT_REMOTE_GATE_DIR:-$root/.cache/remote-gate}"
env_file="${SOYEHT_GATE_ENV_FILE:-$state/gate.env}"
if [[ -f "$env_file" ]]; then
    # The file is code run as this account: ours alone, and not writable by others. A line that
    # fails is a problem of the host (exit 2), never a red main.
    if [[ ! -O "$env_file" ]] ||
        [[ -n "$(find "$env_file" -maxdepth 0 \( -perm -020 -o -perm -002 \) 2>/dev/null)" ]]; then
        echo "remote_gate: $env_file must be owned by this account and not writable by others" >&2
        exit 2
    fi
    # shellcheck disable=SC1090
    if ! (set -eu; . "$env_file") >/dev/null 2>&1; then
        echo "remote_gate: $env_file failed; it must hold only assignments that succeed" >&2
        exit 2
    fi
    set -a
    # shellcheck disable=SC1090
    . "$env_file" || exit 2
    set +a
fi
sha="" steps="" base="" slot="" print_timings=0
while (($#)); do
    case "$1" in
        --sha) sha="${2:?--sha needs a commit}"; shift 2 ;;
        --steps) steps="${2:?--steps needs step ids}"; shift 2 ;;
        --base) base="${2:?--base needs a commit}"; shift 2 ;;
        --slot) slot="${2:?--slot needs a name}"; shift 2 ;;
        --print-timings) print_timings=1; shift ;;
        *) echo "remote_gate: unknown argument $1" >&2; exit 64 ;;
    esac
done
if [[ -n "$slot" && ! "$slot" =~ ^[A-Za-z0-9_-]+$ ]]; then
    echo "remote_gate: bad slot name '$slot'" >&2
    exit 64
fi
tree="$state/tree${slot:+-$slot}"
timings="$state/timings${slot:+-$slot}.json"

if ((print_timings)); then
    cat "$timings"
    exit
fi
[[ "$sha" =~ ^[0-9a-f]{40}$ && -n "$steps" ]] || {
    echo 'remote_gate: --sha FULL_SHA and --steps are required' >&2
    exit 64
}

phase="${SOYEHT_REMOTE_GATE_PHASE:-enter}"

# Run this script again, with the same request, in the Nix shell NIX_FILE and phase PHASE.
reenter() {
    local nix_file="$1" next="$2" command
    command="$(printf 'SOYEHT_REMOTE_GATE_PHASE=%q SOYEHT_REMOTE_GATE_SHELL=%q bash %q --sha %q --steps %q' \
        "$next" "$nix_file" "$self" "$sha" "$steps")"
    [[ -z "$base" ]] || command+="$(printf ' --base %q' "$base")"
    [[ -z "$slot" ]] || command+="$(printf ' --slot %q' "$slot")"
    exec nix-shell "$nix_file" --run "$command"
}

use_nix() {
    [[ "$(uname)" == Linux && -z "${SOYEHT_REMOTE_GATE_NO_NIX:-}" ]] &&
        command -v nix-shell >/dev/null 2>&1 && [[ -e "$root/scripts/linux-gate.nix" ]]
}

# The git to use: SOYEHT_GATE_GIT, else the first on PATH. Exit 2 (the host cannot run the gate).
find_git() {
    if [[ -n "${SOYEHT_GATE_GIT:-}" ]]; then
        git="$SOYEHT_GATE_GIT"
        [[ -x "$git" ]] || {
            echo "remote_gate: SOYEHT_GATE_GIT=$git is not an executable" >&2
            exit 2
        }
    else
        git="$(command -v git || true)"
        [[ -n "$git" ]] || {
            echo "remote_gate: no git on PATH; set SOYEHT_GATE_GIT (or SOYEHT_GATE_ENV_FILE)" >&2
            exit 2
        }
    fi
    # The gate's own steps use git by name.
    export PATH="$(dirname "$git"):$PATH"
}

prepare() {
    find_git
    mkdir -p "$state"
    # One run at a time in a worktree. The lock stays held until the gate exits (the open file
    # survives the exec calls); macOS has no flock, and post_merge.py holds its own lock.
    if command -v flock >/dev/null 2>&1; then
        exec 9>"$tree.lock"
        flock -n 9 || {
            echo "remote_gate: $tree is busy (another run holds $tree.lock)" >&2
            exit 75
        }
    fi
    if ! "$git" -C "$root" cat-file -e "$sha^{commit}" 2>/dev/null; then
        "$git" -C "$root" fetch --quiet origin main || true
    fi
    if ! "$git" -C "$root" cat-file -e "$sha^{commit}" 2>/dev/null; then
        "$git" -C "$root" fetch --quiet origin "$sha" || {
            echo "remote_gate: cannot fetch $sha" >&2
            exit 2
        }
    fi
    if [[ -e "$tree/.git" ]]; then
        "$git" -C "$tree" checkout --quiet --detach --force "$sha" || exit 2
    else
        "$git" -C "$root" worktree prune
        rm -rf "$tree"
        "$git" -C "$root" worktree add --quiet --detach "$tree" "$sha" || exit 2
    fi
    # Untracked files of an earlier run go; ignored ones (target directories) stay.
    "$git" -C "$tree" clean -fdq
    # The commit's own shell wins over the checkout's, so a change to the toolchain is gated
    # with the toolchain it brings.
    if [[ "$phase" == prepare ]]; then
        local nix_file="$tree/scripts/linux-gate.nix"
        [[ -e "$nix_file" ]] || nix_file="$root/scripts/linux-gate.nix"
        reenter "$nix_file" run
    fi
    run_gate
}

run_gate() {
    cd "$tree"
    export SOYEHT_GATE_TIMINGS="$timings"
    export FORBIDDEN_TERMS="${FORBIDDEN_TERMS:-the-remote-steps-do-not-scan}"
    export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-${SOYEHT_GATE_JOBS:-32}}"
    export RUST_TEST_THREADS="${RUST_TEST_THREADS:-${SOYEHT_GATE_TEST_THREADS:-16}}"
    local gate='exec nice -n "${SOYEHT_GATE_NICE:-15}" bash scripts/check_all.sh --tier full --only "$SOYEHT_GATE_STEPS" ${SOYEHT_GATE_BASE:+--base "$SOYEHT_GATE_BASE"}'
    local -a launcher=(bash -c "$gate")
    if [[ -n "${SOYEHT_REMOTE_GATE_SHELL:-}" ]]; then
        # Caches of a shared host stay inside the state directory. The sccache server outlives
        # the run and keeps the temporary directory it started with, which therefore must not
        # be the Nix shell's own (removed when the shell exits).
        export CARGO_HOME="${CARGO_HOME:-$state/cargo-home}"
        export SCCACHE_DIR="${SCCACHE_DIR:-$state/sccache}"
        export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-20G}"
        export UV_CACHE_DIR="${UV_CACHE_DIR:-$state/uv-cache}"
        # Short: the engine tests put Unix sockets under it (a path is limited to 107 bytes, and
        # the tests add more than 50), which a state directory in a home folder leaves too little for.
        export TMPDIR="${SOYEHT_GATE_TMPDIR:-/tmp/soyeht-gate-$(id -u)}"
        mkdir -p "$CARGO_HOME" "$SCCACHE_DIR" "$UV_CACHE_DIR"
        # Engine test sockets live in it: private to this account, and refused when it is not ours.
        mkdir -p -m 700 "$TMPDIR"
        if [[ ! -O "$TMPDIR" ]]; then
            echo "remote_gate: $TMPDIR is not owned by this account" >&2
            exit 2
        fi
        chmod 700 "$TMPDIR"
        if command -v soyeht-gate-fhs >/dev/null 2>&1; then
            # Inside that root rustc is a link into the store; the sccache server must see the
            # real path.
            gate='export RUSTC="$(readlink -f "$(command -v rustc)")"; '"$gate"
            launcher=(soyeht-gate-fhs -c "$gate")
        fi
    fi
    echo "remote_gate: $sha steps $steps"
    local code
    set +e
    # Without the lock descriptor (9>&-): the sccache server and any test process that outlives the
    # gate would otherwise keep the worktree locked.
    SOYEHT_GATE_STEPS="$steps" SOYEHT_GATE_BASE="$base" "${launcher[@]}" 9>&-
    code=$?
    set -e
    if [[ -s "$timings" ]] && command -v python3 >/dev/null 2>&1; then
        python3 - "$timings" <<'PY' || true
import json, sys

for step in json.load(open(sys.argv[1]))["latest"]["steps"]:
    print(f"remote_gate: {step['seconds']:.1f}s {step['status']} {step['id']}")
PY
    fi
    echo "remote_gate: gate-timings $timings"
    echo "remote_gate: exit $code"
    exit "$code"
}

case "$phase" in
    enter)
        if use_nix; then
            reenter "$root/scripts/linux-gate.nix" prepare
        fi
        prepare
        ;;
    prepare) prepare ;;
    run) run_gate ;;
    *)
        echo "remote_gate: unknown phase '$phase'" >&2
        exit 64
        ;;
esac
