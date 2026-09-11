# Armazenamento do engine

`database::Store` abre `soyeht.db`, aplica migrações e executa a transação
de estado, eventos e resultado idempotente. O banco usa WAL, synchronous=FULL
e foreign keys. O engine é seu único escritor. IDs de eventos usam o inteiro
positivo do SQLite; offsets de terminal usam o domínio u64 do protocolo.

`snapshot` lê estado e cursor juntos. `replay` usa cursor exclusivo e recusa
um cursor antigo antes de devolver qualquer evento. A limpeza mantém no
máximo dez mil eventos ou sete dias, sem perder o contador quando todos os
eventos antigos saem. Resultados idempotentes têm retenção de 24 horas.

`segments::Log` mantém a tela da pane. O formato começa com `SOYPT001`.
Cada registro tem tipo u8, offset u64, tamanho u32, payload e CRC32 u32;
inteiros são big-endian. Tipo zero carrega bytes e tipo um carrega o fim de
uma lacuna como u64. O máximo de dados por registro é 64 KiB. A gravação
sincroniza antes de retornar.

A recuperação só descarta uma cauda incompleta no último arquivo. Corrupção
de checksum, formato inválido ou lacuna sem marcador são erros. A limpeza
remove segmentos antigos inteiros e sincroniza o diretório. Não há um log
plano paralelo.

```sh
cargo test --manifest-path engine/Cargo.toml -p soyeht-store
```

Os testes injetam falha dentro de uma transação, banco cheio, versão futura,
cauda truncada e corrupção de checksum. Também verificam backup, idempotência,
cursor, rotação de 512 KiB e retenção de 32 MiB.
