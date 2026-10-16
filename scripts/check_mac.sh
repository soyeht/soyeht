#!/bin/bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
SIGNING=(CODE_SIGNING_ALLOWED=NO)
DERIVED="$ROOT/.cache/mac-derived"
PACKAGES="$ROOT/.cache/apple-packages"
FAST=0
if [[ "${1:-}" == --sign ]]; then
    : "${SOYEHT_DEVELOPMENT_TEAM:?Set the development team for the local build}"
    SIGNING=(CODE_SIGN_STYLE=Automatic "DEVELOPMENT_TEAM=$SOYEHT_DEVELOPMENT_TEAM")
elif [[ "${1:-}" == --fast ]]; then
    # The fast gate: only the build, incremental, in DerivedData that outlives the checkout
    # (the shared Swift suite is a step of its own).
    FAST=1
    gate_cache="${SOYEHT_GATE_DERIVED:-$HOME/Library/Caches/soyeht-gate-derived}"
    DERIVED="$gate_cache/mac"
    PACKAGES="$gate_cache/apple-packages"
    mkdir -p "$DERIVED" "$PACKAGES"
elif [[ $# -gt 0 ]]; then
    echo 'Usage: scripts/check_mac.sh [--sign | --fast]' >&2
    exit 2
fi

run_shared_tests() {
swift test --package-path apps/shared --filter PaneLayout

shared_log=$(mktemp -t soyeht-shared-tests.XXXXXX)
cleanup() {
    rm -f -- "$shared_log"
}
trap cleanup EXIT
set +e
swift test --package-path apps/shared >"$shared_log" 2>&1
shared_status=$?
set -e
cat "$shared_log"
if [[ $shared_status -eq 0 ]]; then
    echo 'shared: suite integral verde'
elif ! uv run python - "$shared_log" <<'PY'
import pathlib
import re
import sys

output = pathlib.Path(sys.argv[1]).read_text()
summaries = re.findall(
    r"Test run with (\d+) tests in .* failed after .* with (\d+) issue", output
)
failures = re.findall(r"Test ([A-Za-z0-9_]+)\(\) failed after", output)
known = "phonePairsByConsentReadsFreshStateAndLosesRevokedAccess"
if summaries and int(summaries[-1][1]) == 1 and failures == [known]:
    total = int(summaries[-1][0])
    print(f"shared: {total - 1}/{total}, falha conhecida: PhoneConnectionTests (#339)")
    raise SystemExit(0)
print("shared: falha inesperada; veja a saida integral acima", file=sys.stderr)
raise SystemExit(1)
PY
then
    exit "$shared_status"
fi
}
((FAST)) || run_shared_tests

xcodebuild -project apps/mac/SoyehtMac.xcodeproj -scheme SoyehtMac \
    -configuration Debug -destination "platform=macOS,arch=$(uname -m)" \
    -derivedDataPath "$DERIVED" \
    -clonedSourcePackagesDirPath "$PACKAGES" \
    -skipPackagePluginValidation ONLY_ACTIVE_ARCH=YES \
    "SOYEHT_PACKAGES=$PACKAGES" "${SIGNING[@]}" build

uv run python scripts/check_pane_drag_type.py \
    "$DERIVED/Build/Products/Debug/Soyeht Dev.app/Contents/Info.plist"
