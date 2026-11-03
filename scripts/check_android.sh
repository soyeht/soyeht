#!/bin/bash
# Builds the Android app (ADR 0134): the Rust core for both Android ABIs, the Kotlin bindings written
# from it, and the debug APK; with --test, runs the Gradle unit tests against the built core.
#
#   scripts/check_android.sh          the build (the gate's `android-app:build`)
#   scripts/check_android.sh --test   the unit tests of the app (`android-app:test`, after the build)
#
# The Rust core's own format, lint and tests are steps of check_all.sh (`android-core:*`). These are
# full-tier steps (area android_app): a Gradle build, two NDK builds and the first download of the
# Android libraries are too much for the fast tier. Nothing here starts an emulator or a window;
# installing the APK on one is in `docs/apps-android.md`.
#
# It needs an Android SDK with the NDK below (ANDROID_HOME, else ~/Library/Android/sdk or
# ~/Android/Sdk), JDK 17 (JAVA_HOME, else Homebrew's or the system's), and the two Android Rust
# targets (`rustup target add aarch64-linux-android x86_64-linux-android`). Gradle 8.10.2 is the
# one on PATH when it is that version, else the distribution this script downloads once into
# .cache/ and checks against the published checksum.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
MODE=build
case "${1:-}" in
    "") ;;
    --test) MODE=test ;;
    *) echo 'Usage: scripts/check_android.sh [--test]' >&2; exit 2 ;;
esac

NDK_VERSION=27.0.12077973
GRADLE_VERSION=8.10.2
GRADLE_SHA256=31c55713e40233a8303827ceb42ca48a47267a0ad4bab9177123121e71524c26
ABIS=(aarch64-linux-android:arm64-v8a x86_64-linux-android:x86_64)
APP="$ROOT/apps/android"
GENERATED="$APP/app/build/generated"

fail() { echo "check_android: $*" >&2; exit 1; }

export PATH="$HOME/.cargo/bin:$PATH"
for sdk in "${ANDROID_HOME:-}" "$HOME/Library/Android/sdk" "$HOME/Android/Sdk"; do
    [[ -n "$sdk" && -d "$sdk" ]] && { export ANDROID_HOME="$sdk"; break; }
done
[[ -n "${ANDROID_HOME:-}" ]] || fail 'no Android SDK: set ANDROID_HOME'
NDK="$ANDROID_HOME/ndk/$NDK_VERSION"
[[ -d "$NDK" ]] || fail "the SDK has no NDK $NDK_VERSION (sdkmanager \"ndk;$NDK_VERSION\")"
if [[ -z "${JAVA_HOME:-}" ]]; then
    for java in /opt/homebrew/opt/openjdk@17 /usr/local/opt/openjdk@17 /usr/lib/jvm/java-17-openjdk-*; do
        [[ -x "$java/bin/java" ]] && { export JAVA_HOME="$java"; break; }
    done
fi
[[ -x "${JAVA_HOME:-}/bin/java" ]] || fail 'no JDK 17: set JAVA_HOME'
for pair in "${ABIS[@]}"; do
    rustup target list --installed | grep -qx "${pair%%:*}" ||
        fail "missing Rust target ${pair%%:*} (rustup target add ${pair%%:*})"
done

TOOLCHAIN=$(echo "$NDK"/toolchains/llvm/prebuilt/*/bin)
[[ -d "$TOOLCHAIN" ]] || fail "no NDK toolchain in $NDK"
API=24
for pair in "${ABIS[@]}"; do
    triple=${pair%%:*}
    underscored=${triple//-/_}
    upper=$(echo "$underscored" | tr '[:lower:]' '[:upper:]')
    export "CC_$underscored=$TOOLCHAIN/$triple$API-clang"
    export "AR_$underscored=$TOOLCHAIN/llvm-ar"
    export "CARGO_TARGET_${upper}_LINKER=$TOOLCHAIN/$triple$API-clang"
done

# The core: the Kotlin bindings from the host library, then the libraries for the phone.
build_core() {
    cd "$APP"
    cargo build --locked -p soyeht-android
    rm -rf "$GENERATED"
    mkdir -p "$GENERATED/uniffi"
    cargo run --locked --quiet --bin uniffi-bindgen -- generate --no-format \
        --library "$(host_library)" --language kotlin --out-dir "$GENERATED/uniffi"
    for pair in "${ABIS[@]}"; do
        triple=${pair%%:*}
        abi=${pair##*:}
        cargo build --locked --release --target "$triple" -p soyeht-android --lib
        mkdir -p "$GENERATED/jniLibs/$abi"
        cp "$(target_directory)/$triple/release/libsoyeht_android.so" "$GENERATED/jniLibs/$abi/"
    done
    cd "$ROOT"
}

target_directory() {
    (cd "$APP" && cargo metadata --format-version 1 --no-deps |
        uv run python -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
}

# The library of this computer, which the unit tests of the bindings load.
host_library() {
    local directory
    directory=$(target_directory)
    for library in "$directory/debug/libsoyeht_android.dylib" "$directory/debug/libsoyeht_android.so"; do
        [[ -f "$library" ]] && { echo "$library"; return; }
    done
    fail 'the core library was not built for this computer'
}

# Gradle.
gradle_bin() {
    if command -v gradle >/dev/null && gradle --version 2>/dev/null | grep -q "^Gradle $GRADLE_VERSION\$"; then
        command -v gradle
        return
    fi
    local home="$ROOT/.cache/gradle-$GRADLE_VERSION"
    if [[ ! -x "$home/bin/gradle" ]]; then
        local zip="$ROOT/.cache/gradle-$GRADLE_VERSION-bin.zip"
        mkdir -p "$ROOT/.cache"
        curl -fsSL -o "$zip" "https://services.gradle.org/distributions/gradle-$GRADLE_VERSION-bin.zip"
        echo "$GRADLE_SHA256  $zip" | shasum -a 256 -c - >&2 || { rm -f "$zip"; fail 'the Gradle download does not match its checksum'; }
        rm -rf "$home.partial"
        mkdir -p "$home.partial"
        unzip -q "$zip" -d "$home.partial"
        mv "$home.partial/gradle-$GRADLE_VERSION" "$home"
        rm -rf "$home.partial" "$zip"
    fi
    echo "$home/bin/gradle"
}
GRADLE=$(gradle_bin)
VERSION=$(uv run python -c 'import json; print(json.load(open("contracts/release.json"))["productVersion"])')
gradle() {
    "$GRADLE" --project-dir "$APP" --max-workers=4 --console=plain "-PsoyehtVersion=$VERSION" "$@"
}

if [[ $MODE == test ]]; then
    SOYEHT_CORE_LIB_DIR=$(dirname "$(host_library)")
    export SOYEHT_CORE_LIB_DIR
    [[ -d "$GENERATED/uniffi" ]] || fail 'run scripts/check_android.sh first: the bindings are not built'
    gradle :app:testDebugUnitTest
    echo "check_android: tests ok"
    exit 0
fi

build_core
gradle :app:assembleDebug

# The debug APK declares the Dev identity and carries both libraries.
APK="$APP/app/build/outputs/apk/debug/app-debug.apk"
[[ -f "$APK" ]] || fail 'no APK was built'
# The listings are read whole: `grep -q` closing a pipe early would fail it under pipefail.
LISTING=$(unzip -l "$APK")
grep -q 'lib/arm64-v8a/libsoyeht_android.so' <<<"$LISTING" || fail 'the APK has no arm64 library'
grep -q 'lib/x86_64/libsoyeht_android.so' <<<"$LISTING" || fail 'the APK has no x86_64 library'
AAPT=$(ls "$ANDROID_HOME"/build-tools/*/aapt2 | sort | tail -n 1)
BADGING=$("$AAPT" dump badging "$APK")
grep -q "^package: name='com.soyeht.next.dev.android'" <<<"$BADGING" ||
    fail 'the APK must be com.soyeht.next.dev.android'
echo "check_android: build ok"
