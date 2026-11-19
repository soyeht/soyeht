#!/bin/bash
# Builds the two targets of apps/shared that a watch app links, `SoyehtAvatar` and `SoyehtVoice`,
# for the watchOS SDK, once for each slice a watch runs: `arm64` (Series 9 and later, the Ultra 2
# and later and the SE 3) and `arm64_32` (Series 6 to 8, the second-generation SE and the first
# Ultra, where `Int` has 32 bits). The deployment target is the `.watchOS(...)` of
# apps/shared/Package.swift: the triple carries none, so the manifest is the only place it is
# written.
#
# The target (watchOS 11) covers the watches that run watchOS 26 and, as announced, watchOS 27 (it
# drops Series 6 to 8, the SE 2 and the first Ultra, and adds Series 12 and the Ultra 4). The 26.5 SDK
# lists no Series 12 or Ultra 4 (`arm64e` in the 27 beta's device table): that slice is unproven
# here and is checked on a real watch in the watch app's slice (W3).
#
# It uses `swift build --target` with `--sdk` and a triple. `xcodebuild -destination
# generic/platform=watchOS` is not used: it refuses the destination unless the watchOS platform
# component is installed through Xcode, which a SDK on disk does not make true, and `swift build
# --product` would build SwiftTerm, which has no watchOS support and is not a dependency of these
# two targets. A host without the watchOS SDK fails here: a step that skipped would hide a break.
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
sdk="$(xcrun --sdk watchos --show-sdk-path 2>/dev/null)" || {
    echo "check_watchos: the watchOS SDK is not installed (Xcode > Settings > Components)" >&2
    exit 1
}
for arch in arm64 arm64_32; do
    for target in SoyehtAvatar SoyehtVoice; do
        echo "== $target for watchOS $arch"
        swift build --package-path "$repo_root/apps/shared" --target "$target" \
            --scratch-path "$repo_root/.cache/swift-watchos/$arch" \
            --sdk "$sdk" --triple "$arch-apple-watchos"
    done
done
