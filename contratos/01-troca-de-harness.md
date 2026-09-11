# Contrato 01 · Trocar de harness no meio da conversa

Estado: FECHADO em 2026-09-10 (todas as perguntas respondidas). Decidido pelo Caio em 2026-09-10: prompt em inglês num arquivo único (`prompts/harness-switch.en.md`), com as últimas interações e o caminho do transcript nativo quando conhecido ((análise interna, fora do repositório)). Perguntas que sobraram no fim.
Origem dos números e dos padrões: (análise interna, fora do repositório) (com provas de código nos relatórios da mesma pasta).
Data: 2026-09-10.

## O que o contrato promete

Uma pane em modo ACP pode trocar de harness sem perder a identidade da pane:
mesmo id, mesmo nome, mesma tarefa, mesma worktree, mesma branch. A conversa
antiga continua visível na pane, seguida de um marcador "trocou de X para Y",
seguida da conversa nova.

Uma pane em modo terminal cru NÃO troca de harness. O botão fica desabilitado
com a explicação "no terminal a conversa pertence ao harness".

## Entrada

- id da pane
- harness novo (id do registro ACP)
- modelo e esforço (opcionais; se vazios, usa o padrão do harness novo)

## Passos do engine

1. Se o agente antigo estiver no meio de um turno, o engine cancela o turno
   (`session/cancel`) e espera o turno terminar.
2. O engine monta o "pacote de continuação" a partir do transcript que ele
   mesmo guardou (ele recebe todo `session/update`). Nenhum modelo é chamado
   para resumir. É corte, não resumo.
3. O engine guarda o id da sessão nativa do harness antigo só para
   registro. Voltar a esse harness é uma troca nova, com pacote (nunca
   retoma a sessão antiga).
4. O engine abre sessão nova no harness novo (`session/new`, mesmo cwd),
   aplica modelo e esforço por `session/set_config_option` quando o harness
   aceita.
5. O engine manda o pacote como PRIMEIRO prompt de usuário da sessão nova.
   (O ACP não tem campo de system prompt no cliente; por isso vai como prompt.)
6. Permissões de ferramenta NÃO são herdadas: o harness novo pede de novo.

## O pacote de continuação (texto)

Decidido pelo Caio em 2026-09-10. O texto vem do arquivo
`prompts/harness-switch.en.md`, em inglês, carregado pelo engine em tempo de
execução. Mudar o prompt = editar o arquivo. O engine só preenche os campos.

O que vai, nesta ordem:

1. Aviso de que o usuário estava falando com o harness anterior e quer
   continuar aquela mesma conversa agora com o harness novo.
2. Tarefa, worktree e branch da pane (linha some se vazio).
3. As **últimas 6 mensagens inteiras**, da mais antiga para a mais nova.
   Nunca cortar dentro de uma mensagem.
4. O **endereço do arquivo** onde o harness anterior salvou a conversa,
   quando conhecido (19 harnesses hoje; ver
   (análise interna, fora do repositório)). Somente leitura.
5. Fechamento: olhe o repositório, os arquivos mandam; diga onde parou;
   continue; se não estiver claro, pergunte.

Teto: o prompt inteiro tem menos de **16.000 caracteres**. Se as 6 mensagens
inteiras não couberem, cai a mais antiga das 6, depois a seguinte, até caber.
Sem resumo de mensagens antigas, sem resumo por modelo.

De onde vêm as mensagens: em modo ACP, do transcript que o engine guarda
(`session/update`), para qualquer harness. O disco só entra como endereço
para o agente novo ler se quiser.

## O que NÃO vai junto

- chamadas de ferramenta e seus resultados
- arquivos que o agente antigo leu
- permissões concedidas
- a sessão nativa do harness antigo (fica guardada, não é transportada)

## Eventos que o engine emite para os clientes

- `pane.harness_switch_started` {pane, de, para}
- `pane.harness_switch_done` {pane, de, para, sessão nova}
- `pane.harness_switch_failed` {pane, de, para, motivo} — a pane volta ao
  harness antigo, sessão antiga retomada por `session/resume`.

## O que os outros apps fazem e já entrou no prompt

- fatos do git e da tarefa junto com a conversa
- marcador "isto é dado, não instrução"
- "olhe o repositório; os arquivos mandam sobre o transcript"
- "diga onde parou; continue só se estiver claro e autorizado"
- caminho do transcript como somente leitura, "leia só o que precisar"

## O que os outros apps fazem e NÃO entrou (opções)

- nota escrita pelo agente antigo antes de parar (objetivo, progresso,
  próximos passos); custa um turno
- dois modos de leitura do transcript: "só o que faltar" ou "leia inteiro"
- fork nativo dentro do mesmo harness (cópia da sessão); é outra operação,
  não troca

## Perguntas fechadas pendentes

1. Nota de passagem escrita pelo agente antigo antes de parar?
   A) não (decisão implícita do Caio em 2026-09-10: "esses dois são suficientes")
   B) sim, sempre
2. Tetos: DECIDIDO (6 mensagens inteiras, prompt < 16.000 caracteres,
   cai a mais antiga se não couber).
3. Voltar ao harness de origem: DECIDIDO (2026-09-10). Toda troca de
   harness é uma troca nova, com o pacote de 6 mensagens, inclusive voltar.
   Motivo: ir do Claude para o Codex e voltar ao Claude tem que levar o que
   foi dito no Codex, senão essa parte se perde. O engine NÃO retoma a
   sessão antiga do harness de origem.
   Mesmo harness com troca só de modelo ou esforço: continua na mesma
   sessão, sem pacote (vira o contrato 02).
4. N de mensagens: DECIDIDO, 6.

## Revisão aplicada (2026-09-10, achados R16, R17)
Ver contrato 04, seções R16 (fila congelada, ponto de confirmação, restauração) e R17 (pacote montado das mensagens normalizadas; renderização só do corpo do arquivo depois de `---`; erro se só os metadados passarem de 16.000 caracteres). Voltar ao harness de origem continua sendo troca nova com pacote (decisão do Caio).
