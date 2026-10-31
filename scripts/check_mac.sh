#!/bin/bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
SIGNING=(CODE_SIGNING_ALLOWED=NO)
DERIVED="$ROOT/.cache/mac-derived"
PACKAGES="$ROOT/.cache/apple-packages"
if [[ "${1:-}" == --sign ]]; then
    : "${SOYEHT_DEVELOPMENT_TEAM:?Set the development team for the local build}"
    SIGNING=(CODE_SIGN_STYLE=Automatic "DEVELOPMENT_TEAM=$SOYEHT_DEVELOPMENT_TEAM")
elif [[ $# -gt 0 ]]; then
    echo 'Usage: scripts/check_mac.sh [--sign]' >&2
    exit 2
fi

# This script builds the app. It runs no test suite: the shared package's tests are the
# `swift:test-shared` gate step (and `swift test --package-path apps/shared` by hand). Running
# them here too repeated that suite on a loaded host, and a timing-sensitive test failing in
# this step was reported as a failed app build.

xcodebuild -project apps/mac/SoyehtMac.xcodeproj -scheme SoyehtMac \
    -configuration Debug -destination "platform=macOS,arch=$(uname -m)" \
    -derivedDataPath "$DERIVED" \
    -clonedSourcePackagesDirPath "$PACKAGES" \
    -skipPackagePluginValidation ONLY_ACTIVE_ARCH=YES \
    "SOYEHT_PACKAGES=$PACKAGES" "${SIGNING[@]}" build

uv run python scripts/check_pane_drag_type.py \
    "$DERIVED/Build/Products/Debug/Soyeht Dev.app/Contents/Info.plist"
