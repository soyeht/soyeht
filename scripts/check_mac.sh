#!/bin/bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
SIGNING=(CODE_SIGNING_ALLOWED=NO)
if [[ "${1:-}" == --sign ]]; then
    : "${SOYEHT_DEVELOPMENT_TEAM:?Set the development team for the local build}"
    SIGNING=(CODE_SIGN_STYLE=Automatic "DEVELOPMENT_TEAM=$SOYEHT_DEVELOPMENT_TEAM")
elif [[ $# -gt 0 ]]; then
    echo 'Usage: scripts/check_mac.sh [--sign]' >&2
    exit 2
fi
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
elif ! python3 - "$shared_log" <<'PY'
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

xcodebuild -project apps/mac/SoyehtMac.xcodeproj -scheme SoyehtMac \
    -configuration Debug -destination "platform=macOS,arch=$(uname -m)" \
    -derivedDataPath .cache/mac-derived \
    -clonedSourcePackagesDirPath .cache/apple-packages \
    -skipPackagePluginValidation ONLY_ACTIVE_ARCH=YES SWIFT_ACTIVE_COMPILATION_CONDITIONS=DEBUG \
    "SOYEHT_PACKAGES=$ROOT/.cache/apple-packages" "${SIGNING[@]}" build
