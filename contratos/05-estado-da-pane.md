# Contrato 05 · Estado da pane

Estado: PARCIAL, decidido pelo Caio em 2026-09-10. A pane de agente (ACP)
está fechada. A pane de terminal cru fica para um EXPERIMENTO com dados;
este contrato define o que o engine precisa ter pronto para o experimento.
Origem: (análise interna, fora do repositório) (análise interna).

## 1. Estados que os clientes recebem (os dois tipos de pane)
`trabalhando`, `ociosa`, `esperando você` (permissão ou pergunta), `sem
sinal`, `morta`, e **`não sei`** (só na pane de terminal cru, quando a
confiança for baixa; nenhum outro app tem isso e é o que evita chutar).
A frase do card (contrato 03 §6) é calculada em cima disso.

## 2. Pane de agente (ACP) — FECHADO
Vem do protocolo (contrato 04 §8): prompt aceito → `trabalhando`;
`session/request_permission` aberto → `esperando você`; `idle` → `ociosa`;
sem notificação há 90 s com turno aberto → `sem sinal`; processo saiu →
`morta`. Exato; sem leitura de tela.

## 3. Pane de terminal cru — DECISÃO ADIADA PARA EXPERIMENTO
Decisão do Caio: não escolher agora entre gancho, marcador ou regex. Tomar
a decisão com dados, como cientista de dados, depois de medir vários
harnesses. Hipótese dele: um classificador leve em cima da tela deve
funcionar melhor que regras à mão ("igual e-mail: regra não funcionou,
classificador funcionou"). Sinal simples de partida: tela mudando =
trabalhando; tela parada = parou.

Regras já válidas, tiradas dos erros dos outros (issues):
- nunca declarar `ociosa` antes do primeiro sinal de vida; `task_next` e
  loop só mandam prompt depois do primeiro `ociosa` real;
- `esperando você` só com pedido real; qualquer sinal novo limpa;
- cobertura parcial aparece como `não sei`, nunca como `ociosa`;
- se um dia houver gancho, ele obedece ao contrato do harness (stdout JSON,
  tempo curto) e não é gravado na configuração compartilhada do usuário.

## 4. O que o engine precisa ter pronto para o experimento (entra na v1)
1. **Gravação de quadros da pane**: a cada mudança da tela, guardar
   `{pane, harness, versão do harness, t, título OSC, últimas N linhas,
   posição do cursor, processo em primeiro plano, bytes por segundo}`.
   Formato: JSONL por pane, rotacionado; ligado por pane ("gravar para
   experimento").
2. **Rótulo de verdade**: para harnesses com gancho oficial (Claude Code,
   Codex), gravar os eventos do gancho com timestamp no mesmo fluxo, como
   rótulo (`começou`, `chamou ferramenta`, `pediu permissão`, `parou`).
   Para a pane ACP, o próprio protocolo é o rótulo.
3. **Exportar dataset**: comando do engine que junta quadros + rótulos de
   um período em um arquivo para análise fora do engine (Python).
4. **Ponto de encaixe para o detector**: o engine chama uma função
   `estado(quadro) -> (estado, confiança)` por pane; a implementação pode
   ser regex (baseline: regras de tela de referência e as do app Swift atual), o sinal
   "tela mudando", ou o classificador; trocável sem mexer no resto.
5. **Métricas**: precisão e recall por estado e por harness, com destaque
   para `esperando você`; latência da detecção; taxa de `não sei`.

## 5. Plano do experimento (depois, com o Caio)
Gravar horas de vários harnesses; comparar regras de tela de referência, regras do app
atual, sinal "tela mudando" e classificador leve contra os rótulos; decidir
por harness. Aprender com as issues: casos "notificação com dois
significados", "estado grudento", "shell em segundo plano", "prompt de
permissão que parece idle".

## Revisão aplicada (2026-09-10, achado R12)
Enum único para os dois tipos de pane: `iniciando`, `trabalhando`, `ociosa`, `esperando_você`, `cancelando`, `sem_sinal`, `recuperando` (engine reconectando ao ptyd), `morta`, `não_sei`. Cada estado vai com `source` (`protocol | hook | osc | screen | none`) e `confidence` (0–1). `esperando_você` exige pedido aberto; `sem_sinal` não apaga pedido nem libera loop/`task_next`; `iniciando` nunca vira `ociosa` sem sinal real.
