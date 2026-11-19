#!/bin/bash
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
swift build --package-path "$repo_root/contracts/swift"
"$repo_root/contracts/swift/.build/debug/protocol-vectors" "$repo_root/contracts/vectors/messages.json"
if [[ "${1:-}" == --vectors ]]; then exit 0; fi  # the fast gate runs these two lines only
mkdir -p "$repo_root/.cache/swift-ios"
simulator_sdk="$(xcrun --sdk iphonesimulator --show-sdk-path)"
xcrun swiftc -warnings-as-errors -target arm64-apple-ios17.0-simulator \
  -sdk "$simulator_sdk" -emit-module -module-name SoyehtProtocol \
  "$repo_root"/contracts/swift/Sources/SoyehtProtocol/*.swift \
  -emit-module-path "$repo_root/.cache/swift-ios/SoyehtProtocol.swiftmodule"
