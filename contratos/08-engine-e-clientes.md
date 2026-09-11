# Contrato 08 · Engine falando com os clientes

Estado: v2 LIBERADO PARA A FATIA 1 pela revisão da aria em 2026-09-10 ("fatia 1 liberada"); o restante da v1 segue aberto a ajustes. Reescrito depois da revisão da aria
(a revisão interna dos contratos, achados R01, R02, R05, R06, R10, R11, R14, R18, R19,
R20 aplicados aqui). Clientes: app Swift novo (Mac e iPhone), app
TypeScript (Linux e Windows), CLI, e o servidor MCP dos agentes.

## 1. Regras que não mudam
1. **Um contrato só** para todos os clientes. O iPhone é o mesmo cliente
   com outra tela. A CLI e o servidor MCP também são clientes.
2. **O cliente nunca calcula estado.** Pede a foto, recebe eventos, desenha.
3. **Envelope JSON-RPC 2.0 completo** (R19): pedido `{jsonrpc, id, method,
   params}`, resposta `{jsonrpc, id, result}` ou `{jsonrpc, id, error}`,
   evento = notificação `{jsonrpc, method, params}` sem `id`. O número de
   ordem do evento vai DENTRO de `params.eventId`. Erros com `code`
   numérico (faixa -32000..-32099 para os nossos) e `data.kind` com nome
   estável.
4. **Todo quadro no socket local tem cabeçalho comum** (R06):
   `[tipo u8][tamanho u32][corpo]`. Tipo 1 = JSON (UTF-8), tipo 2 = bytes
   de terminal (§6), tipo 3 = anexo (§8). O tamanho é só do corpo. **Todos
   os inteiros binários deste contrato são big-endian**, no cabeçalho e no
   corpo. No WebSocket: quadro de texto = JSON; quadro binário = tipos 2 e
   3, com o mesmo cabeçalho de 5 bytes dentro do quadro. Limites: corpo do
   tipo 1 ≤ 1 MiB (exceto `state.snapshot`, ≤ 16 MiB); corpo do tipo 2 =
   16 bytes de subcabeçalho + até 64 KiB de bytes de terminal; corpo do
   tipo 3 ≤ 256 KiB + subcabeçalho.
5. **Tipos**: ids são strings; inteiros de 64 bits (offsets, tamanhos) vão
   como string decimal; enums são strings fechadas; campo desconhecido é
   ignorado pelo leitor (para permitir versão N+1 falar com N).
6. **Versão do protocolo é um inteiro.** O engine aceita a atual e a
   anterior; o cliente diz a dele no `hello`; o engine responde a que vai
   usar ou fecha com `protocol_version_unsupported`.
7. **Um esquema por método e por evento** (arquivo `contracts/protocol.json`
   no repositório, fonte única; tipos gerados para Rust, Swift e TypeScript;
   CI falha se o código diverge). Este contrato descreve; o esquema define.
   Mínimo do esquema da fatia 1: ids de todos os alvos (`workspaceId`,
   `paneId`, `streamId`, `deviceId`, `requestId`, `promptId`) e o tipo de
   `result` de cada método; vetores de teste de ida e volta para cada
   mensagem e para o cabeçalho binário.
8. **Textos de erro em inglês, com código estável**; quem traduz é o app.

## 2. Transporte, autenticação e papéis (R10, R11)
| Cliente | Caminho | Prova de identidade |
|---|---|---|
| Mac (Swift), Linux/Windows (TS), CLI | socket Unix em `<dados>/engine.sock`; diretório 0700, socket 0600; no Windows, named pipe com ACL do usuário | mesmo usuário do SO + `hello` |
| servidor MCP (um por pane de agente) | mesmo socket local | `hello` com `binding` emitida pelo engine ao criar a pane (segredo curto + paneId); sem `binding` válida, vira cliente "humano local" sem papel |
| iPhone | WebSocket em texto puro na porta do Soyeht nos endereços do Tailscale (contrato 06) | `deviceToken` emitido no pareamento (§2.2) |

Limite declarado: processos arbitrários do mesmo usuário conseguem abrir
o socket local; o isolamento é por usuário do SO, não por processo.

**2.1 Handshake (uma pergunta, uma resposta).**
1. Ao aceitar a conexão, o engine manda a notificação `auth.challenge
   {nonce (32 bytes aleatórios, base64, único por conexão), engineId}`
   antes de qualquer outra mensagem.
2. O cliente manda o pedido `hello {client, clientVersion,
   protocolVersion, deviceToken?, proof?, binding?}`. Para o iPhone,
   `proof = Ed25519(chave privada do aparelho, bytes = "soyeht-hello-v1"
   || nonce || engineId || deviceToken)`. Clientes locais não mandam
   `proof` (a identidade é o usuário do SO); o MCP manda `binding`.
3. O engine responde UMA vez: `welcome {engineVersion, protocolVersion,
   connectionId, role, capabilities, dbId}` ou erro (`unauthorized`,
   `protocol_version_unsupported`) e fecha.
Nenhum outro pedido é aceito antes do `welcome` (resposta `unauthorized`
e conexão fechada). Sem `hello` válido em 5 s, o engine fecha. `role` ∈
`human_local | human_device | agent | chief`. O diálogo de pareamento
(§2.2) usa `pair.*` no lugar do `hello` e tem o prazo do código.

**2.2 Pareamento do iPhone (o caminho até ter `deviceToken`).**
Só na conexão WebSocket, antes do `hello` autenticado, e só enquanto a
janela "Adicionar iPhone" está aberta no Mac:
1. Mac: `device.pair.start {}` → `{code, expiresAt (10 min), uri}`; o app
   mostra o QR com `uri` (endereço + `code`).
2. iPhone: `pair.begin {code, deviceName, devicePublicKey (Ed25519, 32
   bytes, base64)}` → `{requestId, challenge (32 bytes aleatórios, base64)}`.
3. iPhone: `pair.prove {requestId, proof}` com `proof = HMAC-SHA-256(key,
   challenge || deviceName)`, `key = SHA-256(code || devicePublicKey)` →
   `{status: "pending"}`; o Mac recebe `device.pair_requested {requestId,
   deviceName, fingerprint = SHA-256(devicePublicKey) em hex curto}`.
4. Mac: `device.pair.decide {requestId, approve: true|false}`. O engine
   espera o consentimento até o prazo do código (10 min); esse diálogo
   pré-pareamento NÃO está sujeito aos 5 s do `hello`.
5. iPhone recebe `pair.result {requestId, approved, deviceToken?,
   deviceId?, reason?}`; código consumido (uso único). O `deviceToken` fica
   ligado ao `devicePublicKey`: em toda conexão seguinte, o `hello` leva a
   `proof` sobre o `nonce` do `auth.challenge` (§2.1), e o engine confere
   contra a chave registrada. Depois do `pair.result` aprovado, o iPhone
   fecha e reconecta fazendo o handshake normal. Motivos de recusa: `expired`,
   `consumed`, `bad_proof`, `denied`, `revoked`.
`device.revoke {deviceId}` invalida o token, fecha conexões abertas e
apaga os tokens de push daquele aparelho.

**2.3 Push.** `push.register {platform: "apns", token, kind: "alert" |
"live_activity", activityId?}` e `push.unregister`. O engine guarda por
dispositivo; renovação = registrar de novo. Eventos que geram push:
`pane.permission_requested`, `task.moved` para "sua vez",
`pane.state_changed` para `sem_sinal`, `pane.session_lost`.

**2.4 Matriz de papéis** (quem pode o quê):
| Operação | human_local | human_device (iPhone) | agent (MCP da pane) | chief |
|---|---|---|---|---|
| ver tudo do workspace | sim | sim | só a própria tarefa/pane | sim |
| `pane.input` (digitar no terminal cru) | sim | não na v1 (só ler) | não | não |
| `pane.prompt`, `pane.cancel`, permissão, yolo, config | sim | sim | não | não |
| `task.create/comment/subitem/block/release/request_review/complete` | sim | sim | só na própria tarefa | sim |
| `task.next` | não | não | sim | não |
| `task.dispatch`, `task.unblock`, `task.list` | sim | sim | não | sim |
| `pane.role.set` (nomear chefe) | sim | sim | não | não |
| `device.*`, `settings.set`, `experiment.*` | sim | `device.list` | não | não |

## 3. Foto do estado e eventos, sem perder nada (R05)
- **Mudança de estado e evento nascem na mesma transação do banco.** Todo
  evento tem `eventId` crescente, único por `dbId`.
- `state.snapshot {}` (global na v1; sem filtro por workspace) → a foto e o
  `cursor` são lidos **na mesma transação de leitura**; o `cursor` é o
  último `eventId` incluído. A foto traz: workspaces; panes com modo,
  harness, sessão (`sessionId`, `generation`), estado + confiança + frase,
  opções de configuração e operação pendente, fila de prompts (ids e
  origem), loop e contadores, papel; tarefas com estado, selos, bloqueio,
  dono, tentativa ativa, subitens, últimos 20 comentários e `commentCount`;
  pedidos de permissão/elicitação abertos com prazo; caixa de atenção;
  dispositivos; capacidades efetivas por harness; `streams` (§6).
- `events.subscribe {since}` (`since` exclusivo): o engine **registra a
  assinatura antes** de ler o limite do replay, manda os eventos
  `(since, limite]` em ordem, e depois os novos; eventos que chegaram à
  fila viva com `eventId ≤ limite` são descartados dessa fila (já foram no
  replay), sem buraco nem repetição. Se `since` for
  mais velho que o log retido (10.000 eventos ou 7 dias): erro
  `cursor_too_old` ANTES de mandar qualquer evento; o cliente refaz
  `snapshot`. `dbId` diferente do `welcome` anterior = base trocada =
  refazer tudo.
- **Reconexão**: `hello` → `state.snapshot` → `events.subscribe {since:
  cursor}`. Sempre os três.
- Todo pedido que muda estado grava `actor = {role, client, deviceId?,
  paneId?}`; é o "quem moveu" do card.
- **Repetição segura (R09)**: todo método que muda estado EXIGE
  `idempotencyKey` (UUID do cliente, válida por 24 h), com escopo pela
  identidade estável do cliente (`deviceId` no iPhone; usuário do SO +
  nome do cliente no local). Chave e resultado são gravados na MESMA
  transação da mudança de estado. Mesma chave e mesmo conteúdo = mesma
  resposta; mesma chave e conteúdo diferente = `idempotency_conflict`.
  Vale também para `pane.create` e `pane.close` até o `ptyd` (contrato 07):
  cair depois do spawn e antes de gravar não cria segunda pane; `close`
  perdido não ressuscita processo.
- **Cliente lento (R20)**: fila de saída por conexão limitada a 8 MiB;
  eventos de controle têm prioridade sobre bytes de terminal; ao estourar,
  o engine fecha a conexão com `slow_consumer` e o cliente ressincroniza.
  Nunca descarta evento em silêncio; nunca bloqueia a persistência.

## 4. Pedidos (métodos)
Cada método tem esquema próprio de `params` e `result` (nunca só `{ok}`
para consultas). Listas paginadas: `{items, nextCursor?}` com `limit`.

**Workspaces**: `workspace.list`, `workspace.create {name, path}`,
`workspace.rename {workspaceId, name}`, `workspace.close`, `workspace.focus`
(foco é **por cliente**, não global: um toque no iPhone não move o teclado
do Mac).

**Panes**: `pane.create {workspaceId, mode: "terminal"|"agent", harness?,
model?, effort?, permission?, cwd|worktree?, taskId?, name?}` →
`{paneId, binding?}`; `pane.close`, `pane.rename`, `pane.arrange
{workspaceId, layout}` (layout é compartilhado e só muda por este pedido),
`pane.focus` (por cliente), `pane.resize {cols, rows}` (política: o engine
usa o tamanho do último cliente humano que redimensionou),
`pane.input {bytes}` (terminal cru),
`pane.prompt {text, attachments?, idempotencyKey}` → `{promptId, queued}`,
`pane.queue.list`, `pane.queue.remove {promptId}`,
`pane.cancel`,
`pane.permission.answer {requestId, optionId}`,
`pane.elicitation.answer {requestId, fields}` (perguntas com campos; nunca
respondida por yolo),
`pane.config.set {configId, value}` → `{applied: bool, effective, sessionPreserved: bool}`,
`pane.yolo {on}` → `{effectiveMode}` (guarda o modo anterior; §4 do 04),
`pane.switch_harness {harness, model?, effort?}` → `{switchId}`,
`pane.loop.set {prompt, intervalSec, maxRounds, stopWhen}` / `pane.loop.stop`,
`pane.role.set {role: "chief"|"worker"}`,
`pane.auth.start` → abre o login (contrato 04 §7); evento `pane.auth_finished`,
`pane.transcript {before?, limit}` → mensagens normalizadas com `role,
localId, generation, seq, complete`,
`pane.stream.subscribe {paneId, from: "tail" | {streamId, offset}}` /
`pane.stream.unsubscribe` (§6),
`pane.message.send {toPaneId, text}` e `pane.message.list` (mensagens entre
panes, persistidas, com estado `queued|delivered|read`),
`pane.record.set {on}` (não gravar a tela desta pane; contrato 07 §2).

**Tarefas** (mapa 1:1 com as ferramentas MCP do contrato 03 §11):
`task.create`, `task.list {workspaceId, state?, limit, cursor?}`,
`task.show {taskId}` (inclui histórico e tentativas), `task.comments
{taskId, before?, limit}`, `task.update {taskId, version, ...}` (edição
protegida por `version`; conflito = `stale_version`), `task.move {taskId,
state}`, `task.comment`, `task.subitem.add`, `task.subitem.done`,
`task.block {kind, reason}`, `task.release`, `task.request_review`,
`task.complete {summary}`, `task.dispatch {taskId, preset}`,
`task.unblock`, **`task.next {}`** (só papel `agent`; regras no 03 §5).
Ids de tarefa são `{workspaceId, taskId}` sempre juntos.

**Experimento (contrato 05 §4)**: `experiment.record.set {paneId, on}`,
`experiment.export {from, to}` → `{path, bytes}`, `experiment.metrics`,
`experiment.detector.set {name}`.

**Outros**: `attention.list`, `harness.list` (registro + detectados +
capacidades sondadas), `model.list {harness}` (catálogo da sessão quando
existe), `device.pair.start|decide`, `device.list`, `device.revoke`,
`push.register|unregister`, `settings.get`/`settings.set {key, value}`
(chaves tipadas no esquema), `engine.status` → `{version, generation,
ptyd: {connected, version, generation}, db: {id, schemaVersion}}`.

## 5. Eventos (notificações)
Nomes **iguais aos publicados nos contratos 01–04 e 07** (R01); campos em
inglês em todos os contratos (tabela de correspondência no fim). Todos com
`eventId`, `at`, `actor?`.
- `workspace.created|renamed|closed`
- `pane.created|closed|renamed|arranged`
- `pane.session_started {harness, version, sessionId, generation, options}`
- `pane.state_changed {state, confidence, phrase, source}` (enum único do 05)
- `pane.update {kind, ...}` (união fechada: `agent_message_chunk`,
  `agent_thought_chunk`, `tool_call`, `tool_call_update`, `plan`,
  `usage`, `terminal_output {terminalId, bytes}`, `config_option_update`)
- `pane.turn_ended {promptId, stopReason}`, `pane.queued {n, promptIds}`
- `pane.permission_requested {requestId, generation, title, detail,
  options: [{optionId, kind, name}], expiresAt}`,
  `pane.permission_answered {requestId, result: selected|cancelled|expired,
  optionId?, by}`
- `pane.elicitation_requested {requestId, schema}`, `pane.elicitation_answered`
- `pane.config_changing {configId, value}`, `pane.config_changed
  {configId, value, options, sessionPreserved}`, `pane.config_failed
  {configId, value, reason}`
- `pane.harness_switch_started {switchId, from, to}`,
  `pane.harness_switch_done {switchId, from, to, sessionId, generation}`,
  `pane.harness_switch_failed {switchId, from, to, reason, restored: bool}`
- `pane.session_lost {reason, recovery: resumed|new_session_with_package|needs_login}`
- `pane.needs_login`, `pane.auth_finished {ok}`
- `loop.tick {paneId, round, of}`, `loop.stopped {paneId, reason}`
- `task.created`, `task.moved {actor, from, to, reason}`, `task.claimed
  {paneId, attempt}`, `task.released {reason, attempt}`, `task.commented`,
  `task.subitem_changed`, `task.blocked {kind, reason}`, `task.unblocked`,
  `task.phrase_changed {phrase}`, `task.updated {version}`,
  `task.attention {workspaceId, n}`
- `attention.changed {workspaceId, count, items}`
- `pane.message {fromPaneId, toPaneId, text, state}`
- `device.pair_requested`, `device.paired`, `device.revoked`
- `engine.updating`, `engine.restarted {generation}`, `ptyd.reconnected
  {generation}`, `ptyd.lost`

## 6. Fluxo de bytes do terminal (R18)
- `pane.stream.subscribe` → `{streamId, generation, firstRetainedOffset,
  nextOffset, binaryPaneRef (u32 usado no cabeçalho)}`. `offset` = índice do
  próximo byte a ler no fluxo; um processo novo na pane muda `generation`
  e zera o offset.
- Quadro tipo 2: `[binaryPaneRef u32][generation u32][offset u64][bytes]`,
  bytes ≤ 64 KiB.
- `from: "tail"` → o engine, sob o mesmo trinco em que registra a
  assinatura viva, captura `H = nextOffset` naquele instante e calcula
  `start = max(firstRetainedOffset, H − 2 MiB)`, ajustado PARA A FRENTE
  até a próxima fronteira de registro (no máximo 64 KiB adiante; a janela
  fica entre 2 MiB − 64 KiB e 2 MiB). Manda `stream.replay_start {streamId,
  generation, fromOffset: start, toOffset: H, reset: true}`, entrega
  `[start, H)` INTEIRO em quadros, `stream.replay_done {nextOffset: H}`,
  e depois o vivo a partir de H. Barreira: nenhum byte vivo antes de
  `replay_done`; nenhum salto entre o fim do replay e o vivo. O cliente
  reseta o terminal antes de aplicar um replay com `reset: true`.
- `from: {streamId, offset}` válido e retido → `stream.replay_start
  {reset: false}` e entrega TUDO desde ali, paginado em quadros, sem
  buraco, SEM resetar a tela (é um delta). Fora do retido ou geração
  diferente → `resync_required {firstRetainedOffset, generation}` e o
  cliente pede `tail`.
- O replay começa em fronteira de gravação (registro do segmento); uma
  sequência de escape pode estar dividida entre dois registros. Isso é
  esperado: o analisador de terminal do cliente trata continuação. Um
  `tail` começa no início da janela retida e o cliente já resetou a tela,
  então a reconstrução é a mesma que a de um terminal que leu tudo desde ali.
- Buraco na retenção do `ptyd` (contrato 07): o engine grava um registro
  `gap {fromOffset, toOffset}` no segmento e o replay entrega
  `stream.gap {fromOffset, toOffset}`; o cliente mostra uma linha
  "trecho perdido" em vez de fingir continuidade.
- Terminais abertos pelo agente (contrato 04 §5) chegam por `pane.update
  {kind: terminal_output, terminalId}`, não por este fluxo.

## 7. Erros
`error: {code, message, data: {kind, ...}}`. `kind` estável:
`not_found`, `invalid_params`, `unauthorized`, `forbidden_role`,
`pane_busy`, `task_has_owner`, `transition_refused {reason}`,
`stale_version`, `idempotency_conflict`, `harness_unavailable`,
`needs_login`, `cursor_too_old`, `resync_required`, `slow_consumer`,
`protocol_version_unsupported`, `disk_full`, `ptyd_unavailable`.

## 8. Limites (R20) — tabela única, vale sobre qualquer outra menção
| O quê | Limite |
|---|---|
| corpo JSON (tipo 1), qualquer mensagem, inclusive partes do snapshot | ≤ 1 MiB |
| corpo binário de terminal (tipo 2) | 16 B de subcabeçalho + ≤ 64 KiB de bytes |
| corpo de anexo (tipo 3) | 16 B de subcabeçalho + ≤ 256 KiB, em pedaços com `attachmentId` |
| `pane.transcript` por página | ≤ 200 mensagens ou 512 KiB |
| comentários dentro do snapshot | 20 por tarefa (o resto por `task.comments`) |
| log de eventos retido | 10.000 eventos ou 7 dias |
| fila de saída por conexão | 8 MiB (controle antes de terminal); estourou = `slow_consumer` |
| conexões por dispositivo | 8 |
| replay de `tail` | 2 MiB |

**Snapshot em partes (sem ciclo de `slow_consumer`).** `state.snapshot`
não devolve a foto na resposta. O engine abre UMA transação de leitura,
manda a foto em notificações `state.snapshot_part {snapshotId, index, of,
section, data}` (cada uma ≤ 1 MiB; seções: workspaces, panes, tasks,
requests, attention, devices, capabilities, streams), e só então responde
ao pedido com `{snapshotId, cursor, parts: of}`. As partes NÃO entram no
orçamento de 8 MiB da fila: o engine só enfileira a parte seguinte depois
que a anterior foi escrita no socket (ritmo do leitor). Se a conexão cair
no meio, o cliente refaz o pedido; partes de um `snapshotId` antigo são
descartadas.

## 9. Tabela de correspondência de nomes (R01)
| Nos contratos 01–07 | Nome canônico (esquema) |
|---|---|
| `de`, `para`, `motivo`, `ator` | `from`, `to`, `reason`, `actor` |
| `tipo` (bloqueio) | `kind` |
| `frase` | `phrase` |
| `pane.attention {n}` / `task.attention {n}` | `task.attention {workspaceId, n}` (por tarefa) e `attention.changed` (caixa) |
| `pane.loop.*` | `loop.tick`, `loop.stopped` (com `paneId`) |
| estados da pane | enum único do contrato 05 §1 |

## 10. Fora da v1
Mais de um Mac no mesmo engine; relay na nuvem; HTTPS via `tailscale
serve`; assinatura de eventos por workspace; digitar no terminal cru pelo
iPhone.

## Decisões aplicadas às duas perguntas antigas
1. MCP = cliente separado e fino; regras e persistência no engine;
   `task.next` existe no engine (papel `agent`).
2. Terminal no mesmo socket, com cabeçalho comum de tipo e tamanho.
