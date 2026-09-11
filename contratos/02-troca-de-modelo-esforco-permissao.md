# Contrato 02 · Trocar modelo, esforço ou permissão no meio da conversa (mesmo harness)

Estado: FECHADO em 2026-09-10 (decisões do Caio no fim).
Origem: (análise interna, fora do repositório) seção 4 de cada app, protocolo ACP v1/v2 e o adaptador ACP do Claude Code. Data: 2026-09-10.

## O que o contrato promete

Trocar modelo, esforço ou permissão no mesmo harness **continua na mesma
sessão**. Sem pacote de continuação, sem sessão nova. A conversa segue.

Vale para pane em modo ACP. Em modo terminal cru não há troca no meio: o
valor só entra no lançamento, por flag.

## Como funciona no ACP (prova)

1. Quando o engine abre a sessão (`session/new`), o agente devolve uma lista
   de "opções de configuração", cada uma com id, nome, valor atual e valores
   possíveis (`SessionConfigOption`, schema v1 `agent.rs:2328`).
2. Para mudar, o engine manda `session/set_config_option` com o id da opção
   e o valor (`SetSessionConfigOptionRequest`, `agent.rs:2524`). O agente
   responde com a lista atualizada. Isso é a confirmação.
3. O adaptador ACP do Claude Code expõe exatamente essas opções: `mode`
   (permissão), `model`, `effort`, `fast` (adaptador ACP oficial do Claude Code).
   Aplica ao vivo, sem reiniciar.
4. Na v1 do protocolo existe também `session/set_mode` para permissão; na v2
   isso virou opção de configuração comum (só `session/set_config_option`).
   O engine fala os dois: `set_config_option` sempre; `set_mode` só se o
   agente v1 não listar `mode` nas opções.

## Regras do engine

- **Lista de modelos e esforços: primeiro o que o harness anuncia.** Se o
  harness diz quais modelos e esforços tem (opções de configuração do ACP,
  ou comando de listagem do harness), o seletor mostra isso. Só se o harness
  não anuncia nada, o seletor mostra a lista nossa, à mão, por harness.
- **Se o harness não aceita trocar no meio da conversa**, o engine abre uma
  sessão nova sozinho com o valor novo e manda o pacote de continuação da
  troca de harness (6 mensagens inteiras + endereço do arquivo + aviso). O
  usuário vê na pane um marcador "modelo trocado, conversa reiniciada".
- **Mostrar o valor novo só depois da confirmação** do agente (resposta do
  `set_config_option`). Enquanto espera, o controle fica em "aplicando".
  Se der erro, volta ao valor antigo e mostra o erro.
- **A pane guarda harness + modelo + esforço + permissão juntos.** Nunca id
  de sessão sem o harness (erro encontrado num dos apps de referência).
- **Tarefa com harness/modelo/esforço:** ao abrir a pane para a tarefa, o
  engine aplica os três logo depois do `session/new`, pelas mesmas opções.
- **Se o agente trocar o id da sessão** ao aplicar (alguns adaptadores
  reiniciam o processo por dentro), o engine trata como perda de contexto e
  manda o pacote do contrato 01 (6 mensagens + endereço) na sessão nova.

## O que os apps de referência fazem (resumo, provas na análise interna)

- Seis apps estruturados trocam modelo no meio; nenhum troca harness.
- Onde o harness aceita ao vivo (Claude SDK, Codex por turno, Pi, OpenCode,
  ACP): aplicam sem reiniciar.
- Onde não aceita (alguns adaptadores de processo por turno): reiniciam e
  retomam por cursor; um deles re-alimenta o transcript como texto quando o
  estado nativo se perde.
- Um app só habilita o seletor quando o agente anuncia suporte, e só marca
  sucesso com a confirmação do agente, não com o envio.
- Os apps de terminal digitam `/model` e `/effort` na TUI; não serve para o
  Soyeht em ACP.

## Eventos para os clientes

- `pane.config_changing` {pane, opção, valor}
- `pane.config_changed` {pane, opção, valor, opções atualizadas}
- `pane.config_failed` {pane, opção, valor, motivo}

## Decisões do Caio (2026-09-10)

1. Lista de modelos: **C**, o harness anuncia; lista à mão só para quem não
   anuncia. "Se o próprio harness diz quais modelos e quais esforços ele
   tem, melhor ainda."
2. Harness que não aceita trocar no meio: **B**, o engine abre sessão nova
   e manda o pacote de 6 mensagens, "funciona mais ou menos da mesma forma".

## Como os apps de referência montam a lista de modelos (provas na análise interna)

- Claude Code: pelo SDK (`supportedModels()`) num app; rodando o comando da
  CLI e lendo a saída em outro; lista fixa em dois.
- Codex: pelo app-server (`model/list`, que também devolve os esforços
  aceitos e o esforço padrão).
- Agentes ACP: pelas opções de configuração da sessão (categoria `model`)
  ou pelo campo `availableModels`; um app sobe o agente por 2 a 5 segundos
  só para perguntar o catálogo e fecha.
- Grok: comando `grok models`. Cline: lendo o arquivo de configuração.
- Gemini, Amp, Muse: lista fixa à mão.
Ou seja, todo mundo já faz o C na prática.

## Revisão aplicada (2026-09-10, achados R01, R15, R16, R25)
Eventos canônicos: `pane.config_changing {configId, value}`, `pane.config_changed {configId, value, options, sessionPreserved}`, `pane.config_failed {configId, value, reason}`. Yolo e "aceitar edições" seguem o contrato 04 R15. Erro de parâmetro ou timeout ao aplicar NÃO reinicia a sessão (04 R25); só `não_suportado` comprovado aciona a sessão nova com pacote (decisão B do Caio).
