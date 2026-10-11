#!/bin/bash
# Runs the engine check for another operating system inside a VMPal VM, from a Mac.
#
#   scripts/vmpal_check_engine.sh linux   "Ubuntu VM 1"
#   scripts/vmpal_check_engine.sh windows "Windows VM 1"
#
# It sends the committed engine and desktop client sources (`git archive HEAD`, never
# `target/` or local changes) and unpacks them into a source folder of their own, named after
# the commit: `soyeht-check/src-<short sha>` in the VM account's home. A run
# never sees a file of an earlier commit, so a deleted or moved file cannot leak
# into it. Every run shares one build folder, `soyeht-check/target`
# (CARGO_TARGET_DIR), so the dependencies are built once and a second run is
# incremental. Old `src-*` folders stay; delete them in the VM when they are not
# needed. Runs the check script of that commit,
# scripts/check_engine_linux.sh or scripts/check_engine_windows.ps1. The VM needs
# the prerequisites listed in docs/platforms.md. Not a merge-gate step.
set -euo pipefail

usage() {
    echo "usage: $0 linux|windows \"<VM name>\"" >&2
    exit 2
}
[[ $# -eq 2 ]] || usage
target="$1"
vm="$2"

vmpal="${VMPAL_CLI:-/Applications/VMPal.app/Contents/Helpers/VMPalMachine.app/Contents/MacOS/VMPalMachine}"
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
source_folder="soyeht-check/src-$(git -C "$repo_root" rev-parse --short=12 HEAD)"

# The engine and the desktop client read only these paths at build and test time (the
# desktop embeds the fonts and provider icons of the apps).
git -C "$repo_root" archive --format=tar.gz --output="$scratch/soyeht-check.tgz" HEAD \
    engine apps/desktop apps/mac/Resources apps/shared/Sources/SoyehtClient/Resources \
    contracts docs/contracts rust-toolchain.toml scripts
"$vmpal" --cli cp "$scratch/soyeht-check.tgz" "$vm:soyeht-check.tgz"

case "$target" in
linux)
    "$vmpal" --cli exec "$vm" -- /bin/sh -c \
        "mkdir -p '$source_folder' && tar -xzmf soyeht-check.tgz -C '$source_folder'"
    "$vmpal" --cli exec "$vm" --timeout 2h -- /bin/sh -c \
        "CARGO_TARGET_DIR=\"\$HOME/soyeht-check/target\" exec bash '$source_folder/scripts/check_engine_linux.sh'"
    ;;
windows)
    windows_folder="${source_folder//\//\\}"
    "$vmpal" --cli exec "$vm" -- powershell.exe -NoProfile -NonInteractive -Command \
        "New-Item -ItemType Directory -Force '$windows_folder' | Out-Null; tar.exe -xzmf soyeht-check.tgz -C '$windows_folder'"
    # Elevated, because the tests create symbolic links.
    "$vmpal" --cli exec "$vm" --admin --timeout 2h -- powershell.exe -NoProfile -NonInteractive \
        -ExecutionPolicy Bypass -Command \
        "\$env:CARGO_TARGET_DIR = \"\$HOME\\soyeht-check\\target\"; & \"\$HOME\\$windows_folder\\scripts\\check_engine_windows.ps1\""
    ;;
*) usage ;;
esac
