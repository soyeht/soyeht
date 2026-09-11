# Contrato 04 · Sessão com um harness pelo ACP

Estado: FECHADO em 2026-09-10 (quatro decisões do Caio no fim).
Origem: (análise interna, fora do repositório) (protocolo e registro), adaptador ACP do
Claude Code, e como as ferramentas analisadas internamente tratam permissão, cancelamento e
retomada. Data: 2026-09-10. Sem nomes de outros produtos.

## 1. O que o contrato promete

Uma pane em modo agente é uma sessão ACP que o engine abre, alimenta e
observa. O engine é o cliente; o harness é um processo filho falando JSON
por linha. O Mac, o Linux/Windows e o iPhone nunca falam ACP; eles recebem
eventos do engine. A pane de terminal cru continua existindo e não passa
por aqui.

## 2. Ciclo de vida, passo a passo

1. **Lançar.** O engine lê a entrada do harness no registro (comando por
   npx, uvx ou binário, com hash quando existe) ou a configuração do
   usuário, e abre o processo com stdin/stdout em pipe, no `cwd` da
   worktree da pane.
2. **`initialize`.** O engine manda `protocolVersion: 1` primeiro (é o que o
   ecossistema responde hoje); se o agente responder 2, o engine passa a
   falar 2. Manda as capacidades do cliente (ver §5) e o nome "Soyeht".
   Lê `authMethods`; se precisa de login, ver §7.
3. **Sondar.** O que o agente anuncia não é confiável. Depois do
   `initialize`, o engine testa uma vez por impressão digital (harness +
   versão do binário + configuração) os métodos `session/list`,
   `session/resume` (ou `session/load` na v1) e `session/set_config_option`,
   e guarda o resultado real. Repete quando a impressão digital muda.
   Mesmo com o resultado guardado, se na hora der erro, o engine aplica o
   plano B do §6 (sessão nova + aviso). Nunca falha em silêncio.
4. **`session/new`** com `cwd` e a lista `mcpServers` contendo o MCP do
   próprio Soyeht (ferramentas de tarefa, mensagens entre panes, etc.). Isso
   dá a todo agente ACP as ferramentas do Soyeht sem configurar nada.
   Guarda `sessionId` + `cwd` + harness + versão no estado da pane.
5. **Opções.** Lê `configOptions` da resposta (modo de permissão, modelo,
   esforço) e aplica o que a pane ou a tarefa pediu com
   `session/set_config_option` (contrato 02).
6. **`session/prompt`.** Texto do usuário, ou o primeiro prompt da tarefa,
   ou o pacote de continuação (contrato 01), como blocos de conteúdo. O
   agente responde "aceitei" na hora; o resultado vem por notificações.
7. **`session/update`.** O engine guarda tudo no transcript da pane e
   repassa aos clientes: pedaços de mensagem, pensamento, chamadas de
   ferramenta e suas atualizações, plano, opções que o agente mudou, uso de
   tokens, e `state_update`. O turno acaba quando chega `idle` com um
   `stopReason` (`end_turn`, `cancelled`, `max_tokens`, `refusal`…).
8. **`session/request_permission`.** O agente pergunta com um título, o
   que quer fazer, e opções (`allow_once`, `allow_always`, `reject_once`,
   `reject_always`). O engine responde conforme §4.
9. **`session/cancel`.** Notificação. O engine espera o `idle` com
   `cancelled`; se não vier em 10 s, mata o processo e marca a pane.
10. **Retomar** (§6) e **fechar** (`session/close`, depois encerra o
    processo).

## 3. Prompt enquanto o agente está ocupado

DECIDIDO (Caio, 2026-09-10): o engine enfileira. Se o usuário manda texto durante um
turno, o engine guarda e envia quando chegar `idle`. A pane mostra "1 na
fila". Com ⌘. o usuário cancela o turno atual e a fila é enviada em seguida.
(Só alguns harnesses aceitam "direcionar" no meio do turno; o ACP padrão
não promete isso, então não dependemos disso.)

## 4. Permissão

Só existe um mecanismo no protocolo: o agente pergunta, o cliente
responde. "Modo de permissão" é uma opção de configuração do agente, quando
ele oferece (o adaptador do Claude Code oferece `default`, `acceptEdits`,
`plan`, `bypassPermissions`, `dontAsk`).

Regra do engine por modo da pane:
- **pergunta** (padrão): o pedido vira evento `pane.permission_requested`
  com título, detalhe e opções. Aparece na pane no Mac e no iPhone. A
  primeira resposta vence; o engine responde ao agente e avisa o outro
  cliente. Sem resposta em 30 min, o engine responde `cancelled` e a pane
  fica "esperando sua permissão" na caixa de atenção.
- **aceita edições**: aprova sozinho `allow_once` quando o pedido é escrita
  de arquivo dentro da worktree; o resto pergunta.
- **tudo liberado**: responde `allow_once` a tudo, sem perguntar. Também
  aplica a opção `bypassPermissions` do agente quando existe, para ele nem
  perguntar.
- `allow_always` escolhido pelo usuário vale só nesta pane (DECIDIDO,
  Caio 2026-09-10).
- **Botão "yolo" na pane** (pedido do Caio): muda o modo da pane para
  "tudo liberado" no meio da conversa. Por baixo: o engine passa a responder
  `allow_once` a todo pedido, e, quando o agente oferece a opção de modo
  (ex.: `bypassPermissions`), também a aplica com `set_config_option`
  (contrato 02) para o agente nem perguntar. Dá para desligar de volta.
  O botão fica visível na barra da pane e no iPhone.

## 5. Capacidades que o engine oferece ao agente

- `fs/read_text_file` e `fs/write_text_file`: sim, restritos à worktree da
  pane; fora dela, recusa com erro.
- `terminal/*` (o agente pede para o cliente rodar um comando e ler a
  saída): **DECIDIDO (Caio, 2026-09-10): sim.** O comando roda num
  terminal do Soyeht ligado à pane, e a saída aparece como uma linha de
  terminal dentro da conversa. É o que um multiplexador de terminais tem de melhor a
  oferecer; os apps só de chat declaram `terminal: false`.
- Elicitação (perguntas estruturadas ao usuário): sim, vira o mesmo evento
  de pergunta que a permissão.

## 6. Retomar depois de reinício

O engine guarda `sessionId` + `cwd` + harness + versão. Ao subir de novo:
- v2: `session/resume` sem replay (o engine já tem o transcript); pede
  `replayFrom: start` só se o transcript local estiver corrompido.
- v1: `session/load` com o id.
- Se falhar (método não existe, id não encontrado, agente foi atualizado):
  `session/new` + pacote de continuação do contrato 01 + comentário do
  sistema "sessão nativa perdida; continuando com as últimas mensagens".
  Nunca falha em silêncio.

## 7. Login

`authMethods` do `initialize`: tipo `terminal` = o engine abre o mesmo
binário com os argumentos de login numa pane de terminal cru para o usuário
fazer o login, e depois relança o agente; tipo `agent` = `auth/login` pelo
protocolo. Enquanto não logado, a pane mostra "precisa de login" com botão.

## 8. Estado da pane derivado (entrada do contrato 05)

`trabalhando` = entre o aceite do prompt e o `idle`; `esperando permissão`
= pedido aberto; `ociosa` = `idle`; `cancelando`; `sem sinal` = processo
vivo, turno aberto, nenhuma notificação há N s (N = 90, configurável);
`morta` = processo saiu. O `stopReason` e a última mensagem alimentam a
frase de estado do card.

## 9. Eventos para os clientes

`pane.session_started {harness, versão, opções}`, `pane.update {...}` (um
por tipo de `session/update`), `pane.turn_ended {stopReason}`,
`pane.permission_requested {id, título, opções}`, `pane.permission_answered
{id, por: mac|iphone|regra}`, `pane.queued {n}`, `pane.session_lost {motivo}`,
`pane.needs_login`.

## 10. O que fica fora

Terminal cru (outro contrato); `session/fork` (instável); HTTP como
transporte (rascunho no protocolo); o engine como servidor ACP para os
clientes (ideia anotada, não decidida).

## Perguntas fechadas pendentes

1. DECIDIDO: sim, o comando aparece e roda dentro da pane.
2. DECIDIDO: fila.
3. DECIDIDO: "permitir sempre" só nesta pane; botão yolo por pane.
4. DECIDIDO: sondar uma vez por versão do binário e guardar; erro na hora
   cai no plano B (sessão nova + aviso).

## Revisão aplicada (2026-09-10, achados R03, R04, R12, R14, R15, R16, R17, R25)

**R03 · versão 1 e versão 2 são ciclos diferentes; o engine normaliza os dois.**
| | ACP v1 (o que o ecossistema fala hoje) | ACP v2 |
|---|---|---|
| fim do turno | a RESPOSTA do `session/prompt` chega no fim e traz `stopReason` | resposta imediata vazia; fim vem em `session/update` de estado `idle` com `stopReason` |
| cancelar | `session/cancel`; a resposta do prompt chega com `stopReason: cancelled` | idem, mas o `idle`/`cancelled` vem por update |
| retomar | `session/load {sessionId, cwd}` (repete histórico por updates) | `session/resume {sessionId, cwd, replayFrom?}` |
| modo de permissão | `session/set_mode` OU opção de configuração | só opção de configuração |
| arquivos e terminal pedidos pelo agente | `fs/*`, `terminal/*` existem | removidos do núcleo (viram extensões) |
| login | `authenticate` | `auth/login` |
Regra: o engine tem UM ciclo interno (prompt aceito → trabalhando → terminou com motivo) e um adaptador por versão. Na v1, "terminou" = resposta do prompt; nunca esperar `idle`. Ao cancelar na v1, esperar a resposta do prompt (até 10 s) antes de matar. A primeira entrega implementa v1 completa; v2 só quando um harness real responder 2 no `initialize`.

**R04 · reconectar a um harness vivo não é retomar sessão.** Depois de o engine reiniciar (contrato 07), o canal engine↔ptyd entrega ao engine: id da instância do harness, geração, e os pedidos do harness que ficaram sem resposta (permissão, `fs/*`, `terminal/*`) com número de sequência. O engine responde esses pedidos pendentes a partir do que já persistiu; um pedido cujo efeito é ambíguo (ex.: escrita já feita, resposta não enviada) é marcado para reconciliação e nunca reexecutado às cegas. Nesse caminho o engine NÃO chama `session/new` nem `session/resume`. `session/resume` só quando o processo morreu (ptyd reiniciou ou máquina reiniciou).

**R12 · estados.** Enum único (contrato 05 §1): `iniciando`, `trabalhando`, `ociosa`, `esperando_você`, `cancelando`, `sem_sinal`, `recuperando`, `morta`, `não_sei`. `esperando_você` só existe enquanto há um pedido aberto (permissão ou elicitação); nunca deriva de silêncio. `sem_sinal` é suspeita com origem e confiança; não libera trabalho nem apaga pedido aberto. Uma pane nasce `iniciando` e só vira `ociosa` depois do primeiro `idle` real (ou da resposta do primeiro prompt na v1).

**R14 · pedido de permissão tem identidade e prazo.** `requestId` do engine (não o id nativo), ligado à geração e ao turno. Primeira resposta válida vence. Expirou (30 min) ou a sessão foi cancelada/trocada: o engine fecha o pedido, responde `cancelled` ao harness, e publica `pane.permission_answered {result: expired|cancelled}`; a caixa de atenção mostra "permissão expirou", com ação "retomar", nunca a pergunta antiga.

**R15 · yolo, aceitar edições, elicitação.** Yolo: o engine guarda o modo anterior e o modo efetivamente confirmado pelo harness; o botão só aparece "desligado" depois de o harness confirmar a volta (ou de a sessão ser reiniciada, quando o harness não aceita mudar). Aceitar edições: o engine só aprova sozinho quando a opção anunciada tem `kind: allow_once` E o pedido é reconhecidamente escrita de arquivo dentro da worktree; qualquer dúvida pergunta. `allow_once` é a categoria (`kind`), não o id literal. Elicitação (pergunta com campos) é outro pedido, com esquema próprio, nunca respondido por yolo.

**R16 · troca, configuração e cancelamento são serializados por pane.** Durante uma troca de harness: a fila de prompts congela; o pacote de continuação é montado com a fila incluída; o harness antigo só é encerrado depois de o novo aceitar o primeiro prompt; se a aceitação for ambígua (timeout), o engine reconcilia lendo o transcript do novo antes de reenviar. Falha: restaura o antigo se ainda vivo, senão `session/resume`, senão aviso + sessão nova (`pane.harness_switch_failed {restored}`). `pane.config_changed` informa `sessionPreserved`.

**R17 · transcript = mensagens normalizadas, não notificações cruas.** O engine grava: todo prompt que ele aceitou (papel `user`, `promptId`), e as mensagens do agente montadas dos pedaços (papel `assistant`, `localId`, `generation`, `seq`, `complete`). Replay de `session/load`/`resume` reconcilia por identidade (mesma geração e seq) ou substitui o trecho delimitado, nunca duplica. O arquivo `prompts/harness-switch.en.md` só é renderizado do corpo depois de `---`; substituição literal única de cada `{{campo}}`; se os metadados sozinhos passarem de 16.000 caracteres, erro (nunca pacote sem mensagem).

**R25 · sondagem em sessão descartável.** A sondagem do §2.3 roda numa sessão própria, com parâmetros válidos, prazo de 10 s e limpeza; resultados classificados em `funcionou | não_suportado | não_autenticado | inconclusivo`. Só `não_suportado` comprovado ou perda de sessão comprovada aciona o plano B (sessão nova + pacote); timeout ou erro de parâmetro NÃO reiniciam conversa.
