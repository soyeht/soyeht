#!/bin/bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
SIGNING=(CODE_SIGNING_ALLOWED=NO)
DESTINATION='generic/platform=iOS Simulator'
if [[ "${1:-}" == --sign ]]; then
    : "${SOYEHT_DEVELOPMENT_TEAM:?Set the development team for the local build}"
    SIGNING=(-allowProvisioningUpdates CODE_SIGN_STYLE=Automatic "DEVELOPMENT_TEAM=$SOYEHT_DEVELOPMENT_TEAM")
    DESTINATION='generic/platform=iOS'
elif [[ "${1:-}" == --sign-simulator ]]; then
    : "${SOYEHT_DEVELOPMENT_TEAM:?Set the development team for the local build}"
    SIGNING=(CODE_SIGNING_ALLOWED=YES CODE_SIGN_STYLE=Automatic "DEVELOPMENT_TEAM=$SOYEHT_DEVELOPMENT_TEAM")
elif [[ $# -gt 0 ]]; then
    echo 'Usage: scripts/check_ios.sh [--sign | --sign-simulator]' >&2
    exit 2
fi
xcodebuild -project apps/ios/SoyehtPhone.xcodeproj -scheme SoyehtPhone \
    -configuration Debug -destination "$DESTINATION" \
    -derivedDataPath .cache/ios-derived -clonedSourcePackagesDirPath .cache/apple-packages \
    -skipPackagePluginValidation SWIFT_ACTIVE_COMPILATION_CONDITIONS=DEBUG \
    "SOYEHT_PACKAGES=$ROOT/.cache/apple-packages" "${SIGNING[@]}" build
