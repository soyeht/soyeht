# Toolchains

Rust 1.98.1, fixado em `rust-toolchain.toml`. Confirmado no manifesto stable
oficial de 2026-09-03. Não mudar o toolchain padrão global para compilar.

Xcode 26.6, build 17F113; Swift 6.3.3. CI: runner `macos-26` e
`DEVELOPER_DIR=/Applications/Xcode_26.6.app/Contents/Developer`.
Aplicativos exigem macOS 14 ou iOS 17; build iOS usa SDK do Xcode, sem
assinatura de distribuição. Simuladores de outros projetos não são alterados.

Gerador: Python 3.11 ou superior, somente biblioteca padrão.
TypeScript: compilador fixado no lockfile de `contracts/typescript`.
Os comandos reproduzíveis ficam no README de cada componente.

Referências: https://static.rust-lang.org/dist/channel-rust-stable.toml e
https://github.com/actions/runner-images/blob/main/images/macos/macos-26-arm64-Readme.md.
