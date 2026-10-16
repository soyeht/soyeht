#!/bin/bash
# The remote side of scripts/post_merge.py --remote (docs/dev-local.md, ADR 0089).
#
#   remote_gate.sh --sha FULL_SHA --steps ID[,ID...] [--base FULL_SHA]
#   remote_gate.sh --print-timings
#
# Run from a checkout of the repository on a Linux host. It fetches the commit, checks it out
# in a persistent worktree of that checkout (`.cache/remote-gate/tree`, so the build
# directories survive between runs) and runs `check_all.sh --tier full --only STEPS` there.
# The step output goes to stdout and stderr and the exit code is the gate's. Exit 2 means the
# host could not run the gate (the commit cannot be fetched or checked out). The restricted
# vocabulary is never sent to the host: the steps that run here do not scan for it, and a
# placeholder satisfies the gate's check that it is set. One run at a time per checkout
# (post_merge.py holds the lock). `--print-timings` prints the per-step times of the last run.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
state="${SOYEHT_REMOTE_GATE_DIR:-$root/.cache/remote-gate}"
tree="$state/tree"
timings="$state/timings.json"
sha="" steps="" base="" print_timings=0
while (($#)); do
    case "$1" in
        --sha) sha="${2:?--sha needs a commit}"; shift 2 ;;
        --steps) steps="${2:?--steps needs step ids}"; shift 2 ;;
        --base) base="${2:?--base needs a commit}"; shift 2 ;;
        --print-timings) print_timings=1; shift ;;
        *) echo "remote_gate: unknown argument $1" >&2; exit 64 ;;
    esac
done

if ((print_timings)); then
    cat "$timings"
    exit
fi
[[ "$sha" =~ ^[0-9a-f]{40}$ && -n "$steps" ]] || {
    echo 'remote_gate: --sha FULL_SHA and --steps are required' >&2
    exit 64
}

mkdir -p "$state"
if ! git -C "$root" cat-file -e "$sha^{commit}" 2>/dev/null; then
    git -C "$root" fetch --quiet origin main || true
fi
if ! git -C "$root" cat-file -e "$sha^{commit}" 2>/dev/null; then
    git -C "$root" fetch --quiet origin "$sha" || {
        echo "remote_gate: cannot fetch $sha" >&2
        exit 2
    }
fi
if [[ -e "$tree/.git" ]]; then
    git -C "$tree" checkout --quiet --detach --force "$sha" || exit 2
else
    git -C "$root" worktree prune
    rm -rf "$tree"
    git -C "$root" worktree add --quiet --detach "$tree" "$sha" || exit 2
fi
# Untracked files of an earlier run go; ignored ones (target directories) stay.
git -C "$tree" clean -fdq

cd "$tree"
export SOYEHT_GATE_TIMINGS="$timings"
export FORBIDDEN_TERMS="${FORBIDDEN_TERMS:-the-remote-steps-do-not-scan}"
echo "remote_gate: $sha steps $steps"
bash scripts/check_all.sh --tier full --only "$steps" ${base:+--base "$base"}
