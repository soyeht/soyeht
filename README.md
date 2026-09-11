# Soyeht

Soyeht organiza workspaces e panes de terminal. O engine mantém o estado e
os clientes mostram esse estado no Mac e no iPhone.

A fatia 1 inclui protocolo compartilhado, supervisor de processos, armazenamento
durável, engine, CLI e aplicativos novos para Mac e iPhone. Ainda não há release.
Veja [como rodar o perfil Dev novo](docs/dev-local.md), o
[relatório da fatia](docs/reports/fatia-1.md) e o
[roteiro manual](docs/roteiro-manual-fatia-1.md).

## Organização

- `contracts/`: esquema do protocolo, gerador e vetores comuns.
- `engine/`: workspace Rust.
- `apps/mac/` e `apps/ios/`: aplicativos nativos no perfil Dev.
- `apps/desktop/`: reservado para o cliente TypeScript de Linux e Windows.
- `design/`: desenho das telas.
- `docs/contracts/`: comportamento contratado e aceites.
- `docs/adr/`: decisões de implementação.

## Verificar o código

Instale as versões em [docs/toolchains.md](docs/toolchains.md). Na raiz:

```sh
python3 contracts/generate.py --check
python3 -m unittest discover -s contracts -p 'test_*.py'
python3 -m unittest discover -s scripts -p 'test_*.py'
cargo fmt --manifest-path engine/Cargo.toml --all --check
cargo clippy --manifest-path engine/Cargo.toml --locked --workspace --all-targets -- -D warnings
cargo build --manifest-path engine/Cargo.toml --locked --workspace
cargo test --manifest-path engine/Cargo.toml --locked --workspace
bash scripts/check_swift.sh
swift format lint --strict --recursive apps/shared/Sources apps/shared/Tests apps/mac/Sources apps/ios/Sources
swift test --package-path apps/shared
bash scripts/check_mac.sh
bash scripts/check_ios.sh
python3 scripts/e2e.py --report .cache/e2e.json
npm --prefix contracts/typescript ci --ignore-scripts
npm --prefix contracts/typescript run check
npm --prefix contracts/typescript test
```

O check `forbidden-terms` usa um segredo do repositório e falha quando ele
não está disponível. Os testes locais da guarda usam termos sintéticos.
Não copie o segredo para arquivos nem para logs.

Os testes do engine usam processos reais em diretórios temporários próprios.
Eles cobrem reconexão, adoção do mesmo shell, repetição de criação e entrada,
fechamento durável, replay com lacuna e pareamento com revogação. Os seis aceites sob launchd rodam na CI. Os clientes Swift também têm testes
contra processos reais. Reboot físico, câmera e rede móvel seguem o roteiro manual.

Mudanças entram por PR com checks verdes. Não há push direto na main.
A entrega completa e o roteiro de uso são definidos no
[objetivo da fatia 1](docs/contracts/00-fatia-1-objetivo.md).

## Licença

Apache-2.0. Veja [LICENSE](LICENSE).

App Mac no perfil Dev: [build e uso](docs/apps-mac.md).

App iPhone no perfil Dev: [build e pareamento](docs/apps-iphone.md).
