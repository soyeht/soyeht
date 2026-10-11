#!/bin/bash
# Runs the real-harness acceptance (scripts/e2e_real_harnesses.py) for another operating system
# inside a VMPal VM, from a Mac: a real Claude Code reply, a real Codex reply, a pane switched
# between them both ways, and each messaging the other through the Soyeht MCP tools.
#
#   scripts/vmpal_check_real_agents.sh linux   "Ubuntu VM 1"
#   scripts/vmpal_check_real_agents.sh windows "Windows VM 1" [extra e2e_real_harnesses.py arguments]
#
# Opt-in and costly: it uses the VM account's own Claude Code and Codex sign-ins, so sign both in
# first (a harness that is not signed in is skipped by name) and run it only when you mean to
# spend that quota. It never signs in or out and never reads a credential. The prompts are one
# line each.
#
# It sends the committed sources (`git archive HEAD`, as scripts/vmpal_check_engine.sh does) into
# `soyeht-real/src-<short sha>` in the VM account's home, builds the engine programs into the
# shared `soyeht-check/target` folder, and runs the acceptance there. The VM needs the engine
# prerequisites of docs/platforms.md, Node.js 22 or later with npm (the pinned adapters are
# installed from the npm registry), and Python 3.10 or later (`python3` on Linux, `python` on
# Windows). The JSON report is copied to ./real-agents-<target>.json (REPORT to change).
set -euo pipefail

usage() {
    echo "usage: $0 linux|windows \"<VM name>\" [e2e_real_harnesses.py arguments]" >&2
    exit 2
}
[[ $# -ge 2 ]] || usage
target="$1"
vm="$2"
shift 2
extra="$*"

vmpal="${VMPAL_CLI:-/Applications/VMPal.app/Contents/Helpers/VMPalMachine.app/Contents/MacOS/VMPalMachine}"
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
report="${REPORT:-real-agents-$target.json}"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
source_folder="soyeht-real/src-$(git -C "$repo_root" rev-parse --short=12 HEAD)"

git -C "$repo_root" archive --format=tar.gz --output="$scratch/soyeht-real.tgz" HEAD \
    engine contracts docs/contracts rust-toolchain.toml scripts
"$vmpal" --cli cp "$scratch/soyeht-real.tgz" "$vm:soyeht-real.tgz"

case "$target" in
linux)
    "$vmpal" --cli exec "$vm" -- /bin/sh -c \
        "mkdir -p '$source_folder' && tar -xzmf soyeht-real.tgz -C '$source_folder'"
    "$vmpal" --cli exec "$vm" --timeout 2h -- /bin/sh -c \
        "export PATH=\"\$HOME/.cargo/bin:\$HOME/.local/node/bin:\$HOME/.local/bin:\$PATH\" \
         CARGO_TARGET_DIR=\"\$HOME/soyeht-check/target\" SOYEHT_REAL_ACP=1 && cd '$source_folder' \
         && python3 scripts/e2e_real_harnesses.py --build --report \"\$HOME/$report\" $extra"
    "$vmpal" --cli cp "$vm:$report" "./$report"
    ;;
windows)
    windows_folder="${source_folder//\//\\}"
    "$vmpal" --cli exec "$vm" -- powershell.exe -NoProfile -NonInteractive -Command \
        "New-Item -ItemType Directory -Force '$windows_folder' | Out-Null; tar.exe -xzmf soyeht-real.tgz -C '$windows_folder'"
    "$vmpal" --cli exec "$vm" --timeout 2h -- powershell.exe -NoProfile -NonInteractive -Command \
        "\$env:PATH = \"\$HOME\\.cargo\\bin;C:\\Program Files\\LLVM\\bin;\$HOME\\node;\$env:APPDATA\\npm;\$env:PATH\"; \
         \$env:CARGO_TARGET_DIR = \"\$HOME\\soyeht-check\\target\"; \$env:SOYEHT_REAL_ACP = '1'; \
         Set-Location \"\$HOME\\$windows_folder\"; \
         python scripts\\e2e_real_harnesses.py --build --report \"\$HOME\\$report\" $extra; exit \$LASTEXITCODE"
    "$vmpal" --cli cp "$vm:$report" "./$report"
    ;;
*) usage ;;
esac
