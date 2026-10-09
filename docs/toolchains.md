# Toolchains

Rust 1.98.1, pinned in `rust-toolchain.toml`. Confirmed in the official stable
manifest dated 2026-09-03. Do not change the global default toolchain to build.

Xcode 26.6, build 17F113; Swift 6.3.3. CI: `macos-26` runner and
`DEVELOPER_DIR=/Applications/Xcode_26.6.app/Contents/Developer`.
Applications require macOS 15 or iOS 18 (ADR 0083); the iOS build uses the Xcode SDK,
without distribution signing. Simulators belonging to other projects are not altered.

Generator: Python 3.11 or later, standard library only.
TypeScript: compiler pinned in the `contracts/typescript` lockfile.
Reproducible commands are in each component's README.

References: https://static.rust-lang.org/dist/channel-rust-stable.toml and
https://github.com/actions/runner-images/blob/main/images/macos/macos-26-arm64-Readme.md.
