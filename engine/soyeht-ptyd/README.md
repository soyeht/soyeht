# Supervisor de processos

`soyeht-ptyd` segura o pseudoterminal e o processo filho enquanto o engine
está desconectado. O registro fica em `ptyd-registry.json`; a saída retida
fica em memória até o ack, com teto de 8 MiB por processo. Um intervalo
descartado aparece como gap na próxima busca. O engine é o dono da gravação
durável da tela.

O socket e o registro pertencem ao perfil novo em
`~/Library/Application Support/Soyeht.next/dev/`. `--data-dir` permite um
diretório isolado nos testes. O caminho do socket precisa caber no limite
do sistema; os testes usam diretórios exclusivos em `/tmp/soyeht-next-test-*`.

```sh
cargo build --manifest-path engine/Cargo.toml -p soyeht-ptyd
cargo test --manifest-path engine/Cargo.toml -p soyeht-ptyd
python3 scripts/install_ptyd_dev.py
```

A instalação registra apenas `com.soyeht.next.dev.ptyd`, com KeepAlive.
Ela recusa substituir um job já carregado. O binário instalado fica no
diretório `bin` do perfil, separado do binário em compilação.

O canal interno usa pedidos tipados em `wire.rs`. A variante de pipes está
reservada no tipo, mas é recusada nesta fatia. A confirmação de spawn e close
inclui a sincronização do registro. A chave de um processo fechado continua
fechada. Um supervisor antigo não é adotado por PID depois de reiniciar.

Os testes de processo cobrem reconexão com o mesmo PID, tamanho real do PTY,
idempotência, proteção por geração, registro após reinício, exclusão mútua e
saída acima da retenção sem engine conectado.
