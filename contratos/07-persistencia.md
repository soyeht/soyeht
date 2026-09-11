# Contrato 07 · Persistência e sobrevivência a reinícios

Estado: FECHADO em 2026-09-10 (decisões do Caio no fim). Prova de como é
hoje: memória `reference_soyeht_persistence_today_2026_09_09`; comparação
com a análise interna. Regra: manter o modelo de
hoje (launchd + engine + ptyd + segmentos), consertar os pontos frágeis.

## 1. Três processos, três vidas
| Processo | Supervisor | Morre quando | Segura |
|---|---|---|---|
| App (Mac/Linux/Windows/iPhone) | usuário | fecha a janela | nada durável |
| Engine | launchd, KeepAlive | atualização, crash | estado (banco), ACP como CLIENTE, quadro, loop, chefe |
| `ptyd` (supervisor de processos) | launchd, KeepAlive, separado | só reinício da máquina ou `stop` explícito | TODOS os processos de pane: shells, harnesses em terminal cru E harnesses ACP |

**Decisão A (Caio):** o `ptyd` passa a segurar também os harnesses de
agente (ACP). Para PTY ele dá um pseudoterminal; para ACP ele dá pipes
stdin/stdout. O engine fala com o harness ACP através do `ptyd` (socket
local), nunca como processo filho direto. Atualizar o engine = engine
reinicia, reconecta no `ptyd`, e o harness continua o turno sem perceber.
Atualizar o `ptyd` = raro; exige avisar "vai parar os agentes" e só com
todas as panes ociosas, ou o handoff por descritores (fora da v1).

## 2. Um dono, um banco
- **Dono de todo estado durável: o engine.** Apps nunca escrevem arquivo
  durável (fim do `workspaces.json` escrito pelo Swift).
- **Um banco SQLite só** (`soyeht.db`), com `user_version` e migrações
  numeradas, WAL ligado, escrita só pelo engine. Tabelas: workspaces,
  panes, sessões ACP (id nativo, cwd, harness, versão), transcript ACP
  (mensagens), tarefas, subitens, comentários, tentativas, loops,
  dispositivos pareados, eventos (log append-only com id crescente, para
  reconexão e para o iPhone).
- CBOR só onde já é log append-only e assinado (`owner_events`); o resto
  migra para o banco. `ptyd` guarda seu registro de processos em arquivo
  próprio (hoje é só RAM: consertar), para reconectar depois de reinício
  do engine.
- Tela (segmentos de PTY): como hoje, 512 KiB por segmento, 32 MiB por
  pane, replay por offset; **um** formato (aposentar o log plano legado).
  Gravação ligada por padrão, com teto, e botão "não gravar a tela desta
  pane" (segredos).

## 3. O que volta depois de cada evento
| Evento | Terminal cru | Pane de agente (ACP) | Quadro/tarefas | iPhone |
|---|---|---|---|---|
| fechar o app | tudo continua (engine + ptyd vivos) | idem | idem | idem |
| atualizar/crash do engine | processo vive no ptyd; tela replay; nada perdido | harness vive no ptyd; engine reconecta e retoma o fluxo ACP no ponto; nada perdido | banco | reconecta e pede estado inteiro |
| reinício do ptyd (raro) | processo morre; volta shell novo no cwd + tela gravada; agente relançado por id nativo (tabela de comandos de retomada por harness, na análise interna) | harness morre; `session/resume`; se não der, contrato 01 | banco | idem |
| reinício do Mac | idem ao anterior, para tudo | idem | banco | idem |

Regras tiradas das issues dos outros: nunca apagar id de sessão nativa por
tempo; não retomar pane que o usuário fechou de propósito; nunca deixar
geração velha de `ptyd` rodando invisível (uma só, adotada pelo engine
novo, com versão de protocolo no socket).

## 4. Decisões do Caio (2026-09-10)
1. Harness ACP sob o `ptyd`: **A**.
2. Banco: **um SQLite só**, "se for a melhor ideia" — é: é o que 8 dos 10
   apps usam, com migrações.
3. Tela gravada: ligada por padrão, com teto e botão de desligar (assumido,
   não contestado).

## Revisão aplicada (2026-09-10, achados R04, R09, R24)

**Canal engine ↔ ptyd (R04).** Socket local próprio, com versão de protocolo. O `ptyd` guarda, por processo: `instanceId`, `instanceKey` (chave de idempotência do spawn, dada pelo engine), `generation`, tipo (pty | pipes-ACP), cwd, argv, env filtrada, e um buffer de retenção **de 8 MiB por processo** (bytes de terminal ou mensagens ACP) com número de sequência. Dono da retenção: o `ptyd`, só até o `ack`. Dono dos segmentos duráveis (a tela gravada, 512 KiB × 32 MiB por pane): o engine, que os escreve antes de mandar o `ack {seq}`. Se a retenção encher sem `ack` (engine fora por muito tempo), o `ptyd` NÃO trava o processo filho: descarta os bytes mais antigos e grava um marcador `gap {fromSeq, toSeq}`; o engine, ao reconectar, transforma o marcador em registro de buraco no segmento e o cliente vê "trecho perdido" (contrato 08 §6). Lacuna declarada, nunca silenciosa. Spawn é idempotente por `instanceKey` (segundo pedido com a mesma chave devolve o mesmo processo); `close {instanceId}` é idempotente (fechar o que já fechou responde ok e nunca relança). O engine grava "spawn pedido {instanceKey}" ANTES de chamar o `ptyd` e, ao reconectar, reconcilia: processo com essa chave existe → adota; não existe → repete o pedido. Ao reconectar, o engine se apresenta com `engineGeneration` (crescente, persistido no banco); o `ptyd` aceita UM engine por vez e **recusa geração menor do que a última aceita** (`stale_engine_generation`), então um engine antigo não expulsa o atual. O `ptyd` reenvia tudo desde o último `ack` e a lista de pedidos do harness sem resposta. Registro do `ptyd` em arquivo (não só RAM). Versão incompatível: o engine reporta `ptyd_unavailable` e NÃO abre outro `ptyd`; nunca duas gerações de `ptyd` vivas.

**Reconciliação antes de devolver (R09).** Depois de reiniciar, o engine primeiro pergunta ao `ptyd` quais processos estão vivos; só devolve tarefas de panes cujo processo não existe mais. Despacho parcial (tarefa reservada, worktree criada, pane não aberta) é retomado ou desfeito pelas etapas gravadas.

**Durabilidade (R24).** Um dado só é confirmado ao cliente depois do commit no SQLite em modo WAL com **`synchronous=FULL`** (sincroniza no commit; `NORMAL` pode perder commits confirmados em queda de energia), checkpoint periódico. Limite declarado: a garantia é a do armazenamento do Mac (SSD com cache honesto). Disco cheio: mutações duráveis são recusadas com `disk_full` e aviso na caixa de atenção; processos vivos não são mortos. Retenção separada e configurável para: tela (segmentos), transcript ACP, tarefas/histórico (sem limite), dataset do experimento (contrato 05). "Não gravar a tela desta pane" desliga segmentos E dataset dessa pane. Migração: backup automático do banco antes de migrar; engine mais antigo que o banco recusa subir com mensagem clara (sem downgrade silencioso).
