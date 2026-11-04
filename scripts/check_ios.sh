#!/bin/bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
SIGNING=(CODE_SIGNING_ALLOWED=NO)
DESTINATION='generic/platform=iOS Simulator'
DERIVED="$ROOT/.cache/ios-derived"
PACKAGES="$ROOT/.cache/apple-packages"
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
    -derivedDataPath "$DERIVED" -clonedSourcePackagesDirPath "$PACKAGES" \
    -skipPackagePluginValidation \
    "SOYEHT_PACKAGES=$PACKAGES" "${SIGNING[@]}" build

# The generic destination builds the one binary iPhone and iPad share. Fail if the built
# app stops declaring the iPad family or the iPad orientations that multitasking needs.
PLATFORM=iphonesimulator
[[ "$DESTINATION" == 'generic/platform=iOS' ]] && PLATFORM=iphoneos
INFO="$DERIVED/Build/Products/Debug-$PLATFORM/Soyeht Dev.app/Info.plist"
[[ "$(plutil -extract UIDeviceFamily json -o - "$INFO")" == '[1,2]' ]] || {
    echo 'check_ios: the app must target iPhone and iPad' >&2
    exit 1
}
[[ "$(plutil -extract 'UISupportedInterfaceOrientations~ipad' raw -o - "$INFO")" == 4 ]] || {
    echo 'check_ios: iPad must support all four orientations' >&2
    exit 1
}

# The store needs the privacy manifest in the app and the export-compliance answer in Info.plist.
APP=$(dirname "$INFO")
[[ -f "$APP/PrivacyInfo.xcprivacy" ]] || {
    echo 'check_ios: the app must carry PrivacyInfo.xcprivacy' >&2
    exit 1
}
# SwiftTerm calls stat() and fstat(), so the manifest must declare the file-timestamp category.
plutil -extract NSPrivacyAccessedAPITypes json -o - "$APP/PrivacyInfo.xcprivacy" |
    grep -q NSPrivacyAccessedAPICategoryFileTimestamp || {
    echo 'check_ios: PrivacyInfo.xcprivacy must declare NSPrivacyAccessedAPICategoryFileTimestamp' >&2
    exit 1
}
[[ "$(plutil -extract ITSAppUsesNonExemptEncryption raw -o - "$INFO")" == false ]] || {
    echo 'check_ios: Info.plist must answer ITSAppUsesNonExemptEncryption with false' >&2
    exit 1
}
