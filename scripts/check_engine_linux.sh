#!/bin/bash
# Builds and tests the Rust engine on Linux, and the desktop client (apps/desktop):
# format, lints, build and the whole test suite of each. It is not a merge-gate step
# (scripts/check_all.sh stays the only list of those); run it on a Linux host or through
# scripts/vmpal_check_engine.sh.
#
# Needs: a C toolchain (build-essential), `git` (the version-control tests run a
# real repository), `bash`, and rustup (the pinned toolchain in
# rust-toolchain.toml installs itself on first use). The desktop client also needs
# the GPUI build libraries listed in docs/platforms.md; the tests open no window.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
jobs="${SOYEHT_JOBS:-4}"

for tool in cargo cc git bash; do
    command -v "$tool" >/dev/null || {
        echo "check_engine_linux: '$tool' is required but not on PATH" >&2
        exit 1
    }
done

cd "$repo_root/engine"
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -j "$jobs" -- -D warnings
cargo build --locked --workspace -j "$jobs"
cargo test --locked --workspace -j "$jobs"

# The desktop client is its own workspace; its Linux code (Wayland, X11) compiles only here.
cd "$repo_root/apps/desktop"
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -j "$jobs" -- -D warnings
cargo test --locked --workspace -j "$jobs"
echo 'check_engine_linux: passed'
