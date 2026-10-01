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

# The generic destination builds the one binary iPhone and iPad share. Fail if the built
# app stops declaring the iPad family or the iPad orientations that multitasking needs.
PLATFORM=iphonesimulator
[[ "$DESTINATION" == 'generic/platform=iOS' ]] && PLATFORM=iphoneos
INFO=".cache/ios-derived/Build/Products/Debug-$PLATFORM/Soyeht Dev.app/Info.plist"
[[ "$(plutil -extract UIDeviceFamily json -o - "$INFO")" == '[1,2]' ]] || {
    echo 'check_ios: the app must target iPhone and iPad' >&2
    exit 1
}
[[ "$(plutil -extract 'UISupportedInterfaceOrientations~ipad' raw -o - "$INFO")" == 4 ]] || {
    echo 'check_ios: iPad must support all four orientations' >&2
    exit 1
}
