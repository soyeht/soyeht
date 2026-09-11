# 0004 — Transações e segmentos de terminal

Estado: aceito para implementação, 2026-09-11.

## Contexto

O contrato 08 exige que estado, evento e resultado idempotente sejam
confirmados juntos. O contrato 07 exige gravar a saída antes do ack e
recuperar a tela mesmo quando o processo de shell precisa ser recriado.

## Decisão

O engine é o único escritor de SQLite. Cada mutação executa uma transação
IMMEDIATE com alteração de estado, eventos e resultado idempotente. Uma chave
repetida no mesmo escopo retorna o resultado gravado; conteúdo diferente
retorna conflito. O snapshot lê o estado e o cursor na mesma transação.

O banco usa WAL e synchronous=FULL. Cada versão tem migração numerada;
antes de migrar um banco existente, o engine faz backup pela API do SQLite.
Uma versão de banco mais nova é recusada.

Segmentos de terminal usam registros com tipo, offset e tamanho big-endian.
Um registro nunca cruza um segmento de 512 KiB. O engine sincroniza o arquivo
antes de confirmar os bytes ao supervisor. Retenção remove segmentos antigos
inteiros para manter o limite de 32 MiB por pane. Um gap é um registro explícito
com o intervalo perdido. Uma cauda incompleta é detectada durante a recuperação.

O log pertence à pane. Um shell novo recebe uma geração e um streamId novos;
o replay desse stream inclui a tela já gravada da pane. Os offsets do log
continuam crescentes; a sequência zero do processo novo recebe como base o
fim anterior do log. Assim, reiniciar o processo não apaga a tela e não mistura
bytes de duas gerações em voo.

## Motivo

A transação única elimina confirmações que existem só na memória. A API de
backup inclui o WAL. Separar sequência do processo e offset do log permite
preservar a tela sem fingir que um processo sobreviveu ao reinício da máquina.

Referência de implementação: [API de backup](https://docs.rs/rusqlite/latest/rusqlite/backup/index.html).
