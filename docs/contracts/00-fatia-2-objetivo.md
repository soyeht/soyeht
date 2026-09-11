# Objetivo · Fatia 2 do Soyeht novo: a pane de agente pelo ACP

Autossuficiente, para execução até o fim sem perguntas ao Caio. Mesmas
guardas absolutas da fatia 1 (`00-fatia-1-objetivo.md` §0 e §5: nada de
produção, perfil Dev `Soyeht.next/dev`, sem segredos, sem tags, sem nomes de
concorrentes, sem atribuição a assistente, Apache-2.0). Mesma regra de
autonomia: ambiguidade vira ADR e o trabalho segue; merge próprio com CI
verde; revisão da victoria depois. Data: 2026-09-11.

## 0. Contratos que mandam nesta fatia
`04-sessao-acp.md` (inteiro, inclusive "Revisão aplicada": tabela v1/v2,
canal engine↔ptyd para pipes, estados, permissão com identidade e prazo,
yolo/aceitar edições/elicitação, transcript normalizado, sondagem em sessão
descartável), `02-troca-de-modelo-esforco-permissao.md`, `05-estado-da-pane.md`
§§1–2 (só a parte ACP), `07-persistencia.md` (harness sob o ptyd por pipes),
`08-engine-e-clientes.md` (métodos e eventos de pane de agente). O design é
`design/soyeht-desktop.pen`: Tela 2 (pane nova, seletor harness · modelo ·
esforço, permissão) e Tela 5 (pane de agente com chat e pedido de permissão).

## 1. O que construir
1. **Protocolo**: adicionar ao `contracts/protocol.json` (com vetores e tipos
   gerados) os métodos `pane.prompt`, `pane.queue.list`, `pane.queue.remove`,
   `pane.cancel`, `pane.permission.answer`, `pane.elicitation.answer`,
   `pane.config.set`, `pane.yolo`, `pane.auth.start`, `pane.transcript`,
   `pane.draft`, `harness.list`, `model.list`, e `pane.create` com
   `mode: "agent"`; os eventos `pane.session_started`, `pane.update` (união
   fechada do 08 §5), `pane.turn_ended`, `pane.queued`,
   `pane.permission_requested|answered`, `pane.elicitation_requested|answered`,
   `pane.config_changing|changed|failed`, `pane.session_lost`,
   `pane.needs_login`, `pane.auth_finished`; erros `pane_busy`,
   `needs_login`, `harness_unavailable`.
2. **`soyeht-ptyd`**: segundo tipo de processo, `pipes` (stdin/stdout em pipe,
   stderr para arquivo de log do perfil), com a MESMA retenção, `seq`/`ack`,
   `instanceKey`, geração e reconexão do tipo `pty`; o engine lê e escreve
   as mensagens JSON-RPC do ACP por ele; ao reconectar, o `ptyd` reenvia o
   que não foi confirmado e a lista de pedidos do harness sem resposta.
3. **`soyeht-engine`, cliente ACP v1** (`soyeht-acp` como crate própria):
   lançar pela entrada do registro ACP ou por comando configurado; `initialize`
   com `protocolVersion: 1` e capacidades do cliente (`fs` restrito à
   worktree; `terminal` = SIM, rodando num pty do ptyd e mostrando a saída
   na conversa; aceitar 2 só se o harness responder 2, senão adaptador v1);
   sondagem por impressão digital (binário + versão + config) em sessão
   descartável, resultados `funcionou|não_suportado|não_autenticado|inconclusivo`,
   guardados no banco; `session/new` com `cwd` e `mcpServers` vazio nesta
   fatia; opções de configuração lidas e aplicadas; `session/prompt` com fim
   de turno pela RESPOSTA (v1); `session/update` → transcript normalizado
   (mensagens de usuário aceitas + do agente montadas dos pedaços, com
   `generation` e `seq`) → `pane.update` para os clientes; permissão com
   `requestId` próprio, prazo de 30 min, primeira resposta vence, regra por
   modo da pane (pergunta / aceita edições / tudo liberado); elicitação como
   pedido separado; `session/cancel` esperando a resposta do prompt; fila de
   prompts durável com origem e `promptId`, sinal `pane.draft`; yolo com modo
   anterior e confirmação; `session/load` (v1) ao retomar depois de ptyd
   morto, senão sessão nova + `pane.session_lost` com aviso (o pacote de
   continuação é a fatia 4); `pane.needs_login` + `pane.auth.start` abrindo o
   login numa pane de terminal cru do ptyd; estados da pane só do protocolo
   (`iniciando|trabalhando|ociosa|esperando_você|cancelando|sem_sinal|
   recuperando|morta`), `sem_sinal` com carência de 90 s e sem efeito sobre
   pedidos abertos.
4. **Harness de teste**: um agente ACP v1 falso, em Rust, dentro do repo
   (`engine/soyeht-acp/tests/fake-agent`), determinístico, que responde
   prompts, pede permissão, aceita `set_config_option`, suporta `session/load`
   quando configurado e pode ser mandado "morrer". A CI usa só ele. Um teste
   local opcional (`SOYEHT_REAL_ACP=1`) usa o adaptador ACP oficial do Claude
   Code já autenticado no Mac do Caio.
5. **CLI**: `pane create --agent <harness> [--model] [--effort]`, `pane prompt
   <id> "texto"`, `pane cancel`, `pane permission <id> allow|deny`, `pane
   config <id> <chave> <valor>`, `pane yolo <id> on|off`, `harness list`,
   `model list <harness>`, `pane transcript <id>`.
6. **App Mac**: Tela 2 (pane nova: modo terminal ou agente; seletor único
   harness · modelo · esforço como no `.pen`; permissão; pasta/worktree) e
   Tela 5 (chat da pane de agente: mensagens, pensamento recolhido, chamadas
   de ferramenta, saída de terminal do agente em linha, pedido de permissão
   com botões, fila com contador, botão yolo, cancelar, seletores no
   rodapé). Estilo neomórfico do `.pen`.
7. **App iPhone**: ler a pane de agente (mesma vista, só leitura), responder
   permissão, mandar texto, botão yolo; reconexão com snapshot.
8. **CI**: tudo da fatia 1 mais os testes desta fatia; e2e com o agente
   falso sob launchd.

## 2. Aceite (o trabalho só termina quando todos passam)
1. [CI] Abrir pane de agente, mandar prompt: resposta chega em pedaços e
   aparece igual no Mac e no iPhone (dois clientes conectados); transcript
   persistido e paginado.
2. [CI] Permissão: o pedido aparece nos dois clientes; a primeira resposta
   vence; o outro recebe `permission_answered {by}`; sem resposta em 30 min
   (acelerado no teste) → `expired` e caixa de atenção.
3. [CI] Cancelar no meio do turno: resposta do prompt com `cancelled` e pane
   `ociosa` em ≤ 10 s; sem matar processo saudável.
4. [CI] `kill -9` no engine no meio de um turno: o harness continua sob o
   ptyd (mesmo pid), o engine adota, os pedaços seguintes chegam, um pedido
   de permissão feito durante a queda é recuperado e respondido uma vez só.
5. [CI] Trocar modelo e esforço no meio: `config_changing` → `config_changed
   {sessionPreserved: true}`; erro de parâmetro NÃO reinicia a sessão.
6. [CI] Yolo liga e desliga com confirmação do harness; pedido de permissão
   com yolo ligado é aprovado sem aparecer para o usuário; elicitação NUNCA
   é respondida pelo yolo.
7. [CI] Prompt enviado durante um turno fica na fila (`pane.queued`) e sai
   depois do `ociosa`; com `pane.draft` ativo, prompt automático não entra.
8. [CI] ptyd reiniciado à força: harness morre; engine tenta `session/load`;
   agente falso configurado com suporte → transcript continua; sem suporte →
   sessão nova + `session_lost` com aviso; nunca em silêncio.
9. [CI] Sondagem: agente falso que "anuncia" retomada mas responde
   `method_not_found` é registrado como `não_suportado` e não é sondado de
   novo até mudar a versão.
10. [CI] Login: agente falso que responde `auth_required` → `pane.needs_login`;
    `pane.auth.start` abre a pane de terminal; depois `pane.auth_finished` e a
    sessão abre.
11. [local, roteiro] Mesmos fluxos com o adaptador ACP oficial do Claude Code
    no Mac do Caio: prompt real, permissão real respondida pelo iPhone com
    Wi-Fi desligado, troca de modelo, yolo, `kill -9` no engine no meio.

## 3. Fora desta fatia
Tarefas, loop, chefe, MCP do Soyeht dentro da sessão (fatia 3); pacote de
continuação e troca de harness (fatia 4); ganchos/regex no terminal cru
(fatia 5); push (fatia 6); ACP v2 (só quando um harness real responder 2).

## 4. Como trabalhar e entregar
Igual à fatia 1: ordem protocolo → ptyd pipes → cliente ACP + agente falso →
engine → CLI → e2e → app Mac → app iPhone → roteiro; PRs pequenos, CI verde,
merge próprio; ADR para cada dúvida; ao terminar, `docs/reports/fatia-2.md`
e mensagem para a victoria com os PRs.
