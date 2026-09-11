# Soyeht

Soyeht organiza workspaces e panes de terminal. O engine mantém o estado e
os clientes mostram esse estado no Mac e no iPhone.

A implementação está começando pela fatia 1: protocolo, supervisor de
processos, engine, CLI e aplicativos novos. Ainda não há release.
O código disponível inclui o protocolo compartilhado, armazenamento durável,
supervisor de processos, servidor do engine e CLI. Os aplicativos entram nos
próximos passos. Veja [como rodar o perfil Dev novo](docs/dev-local.md).

## Organização

- `contracts/`: esquema do protocolo, gerador e vetores comuns.
- `engine/`: workspace Rust.
- `apps/mac/` e `apps/ios/`: aplicativos nativos introduzidos nos respectivos passos.
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
npm --prefix contracts/typescript ci --ignore-scripts
npm --prefix contracts/typescript run check
npm --prefix contracts/typescript test
```

O check `forbidden-terms` usa um segredo do repositório e falha quando ele
não está disponível. Os testes locais da guarda usam termos sintéticos.
Não copie o segredo para arquivos nem para logs.

Os testes do engine usam processos reais em diretórios temporários próprios.
Eles cobrem reconexão, adoção do mesmo shell, repetição de criação e entrada,
fechamento durável, replay com lacuna e pareamento com revogação. A instalação
sob o gerenciador de serviços e os aplicativos ainda têm aceites separados.

Mudanças entram por PR com checks verdes. Não há push direto na main.
A entrega completa e o roteiro de uso são definidos no
[objetivo da fatia 1](docs/contracts/00-fatia-1-objetivo.md).

## Licença

Apache-2.0. Veja [LICENSE](LICENSE).
