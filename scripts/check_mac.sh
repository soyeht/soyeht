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
xcodebuild -project apps/mac/SoyehtMac.xcodeproj -scheme SoyehtMac \
    -configuration Debug -destination "platform=macOS,arch=$(uname -m)" \
    -derivedDataPath .cache/mac-derived \
    -clonedSourcePackagesDirPath .cache/apple-packages \
    -skipPackagePluginValidation ONLY_ACTIVE_ARCH=YES SWIFT_ACTIVE_COMPILATION_CONDITIONS=DEBUG \
    "SOYEHT_PACKAGES=$ROOT/.cache/apple-packages" "${SIGNING[@]}" build
