#!/bin/bash
# Runs the engine check for another operating system inside a VMPal VM, from a Mac.
#
#   scripts/vmpal_check_engine.sh linux "Ubuntu VM 1"
#
# It sends the committed engine sources (`git archive HEAD`, never `target/` or
# local changes), unpacks it over `soyeht-check/` in the VM account's home, and runs
# scripts/check_engine_linux.sh there. The build products stay in the VM between
# runs, so a second run is incremental. Files deleted from the repository stay in
# the VM folder; delete the folder there for a fully clean run. The VM needs the
# prerequisites listed in docs/platforms.md. Not a merge-gate step.
set -euo pipefail

usage() {
    echo "usage: $0 linux \"<VM name>\"" >&2
    exit 2
}
[[ $# -eq 2 ]] || usage
target="$1"
vm="$2"

vmpal="${VMPAL_CLI:-/Applications/VMPal.app/Contents/Helpers/VMPalMachine.app/Contents/MacOS/VMPalMachine}"
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

# The engine reads only these top-level paths at build and test time.
git -C "$repo_root" archive --format=tar.gz --output="$scratch/soyeht-check.tgz" HEAD \
    engine contracts docs/contracts rust-toolchain.toml scripts
"$vmpal" --cli cp "$scratch/soyeht-check.tgz" "$vm:soyeht-check.tgz"

case "$target" in
linux)
    "$vmpal" --cli exec "$vm" -- /bin/sh -c \
        'mkdir -p soyeht-check && tar -xzmf soyeht-check.tgz -C soyeht-check'
    "$vmpal" --cli exec "$vm" --timeout 2h -- bash soyeht-check/scripts/check_engine_linux.sh
    ;;
*) usage ;;
esac
