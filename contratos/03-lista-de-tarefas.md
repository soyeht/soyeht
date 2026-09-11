# Contrato 03 · Lista de tarefas do workspace

Estado: FECHADO em 2026-09-10 pelo Caio ("pode fechar a lista de tarefas assim"). Onde diz PENDENTE, vale o valor assumido, confirmado por ele.
Origem de cada mecanismo: (análise interna, fora do repositório) e
(análise interna, fora do repositório). Data: 2026-09-10.
Regra deste arquivo: sem nomes de outros produtos.

## 1. O que o contrato promete

Cada workspace tem uma lista de tarefas. Uma tarefa pode ser criada por
você ou por um agente, pode ficar parada na lista, pode ser puxada por um
agente ocioso ou despachada por você, tem um dono, leva harness + modelo +
esforço, mostra uma frase de estado calculada de fatos, aceita arraste, e
nunca fica presa quando a pane que a rodava morre.

## 2. Objetos

### Tarefa
| Campo | Tipo | Obrigatório | Quem preenche |
|---|---|---|---|
| id | `WS-n` por workspace | sim | engine |
| título, descrição | texto | título sim | criador |
| como_se_verifica | texto | não (PENDENTE Q2, assumi "opcional na v1") | criador |
| estado | ver §3 | sim | engine |
| prioridade | urgente / alta / média / baixa | não (média) | criador |
| dono | id de pane/agente ou vazio | não | criador, despacho ou `task_next` |
| quem_roda | harness + modelo + esforço + permissão, ou "padrão do workspace" | não | criador |
| depende_de | lista de ids | não | criador, agente |
| worktree | caminho + branch, criados pelo worktrunk | engine | engine |
| tentativa_atual | número, começa em 1 | engine | engine |
| bloqueio | tipo + motivo (§6) | engine | agente ou você |
| pr | url + estado dos checks + estado da revisão | engine | engine (git/CI) |
| contrato_de_pr | ligado/desligado | não (PENDENTE Q4, assumi "desligado na v1") | criador |
| criado_em, criado_por | data + ator (`você`, `aria`, `chefe`) | engine | engine |

### Subitem (regra adotada pelo Caio)
`{id: WS-5-T01, texto, agente, instruções, estado: aberto|feito}`.
O agente que pega a tarefa cria os subitens ANTES de executar, e marca cada
um ao terminar. Se a tarefa tem subitens, ela só entra em "em revisão"
quando todos estão feitos; sem subitens, entra direto.

### Comentário
`{autor: você|agente|sistema, texto, data}`. O agente reporta progresso por
comentário, não só no chat. O sistema escreve aqui os fatos: quem moveu,
quem puxou, CI falhou, tarefa devolvida.

### Tentativa
Uma linha por vez que uma pane roda a tarefa: `{n, pane, harness, modelo,
início, fim, resultado: concluída|devolvida|falhou, resumo}`. Tarefa e
tentativa são coisas separadas: a tarefa sobrevive à pane.

## 3. Estados e quem move

Estados: `a_fazer`, `em_andamento`, `em_revisão`, `concluída`, `cancelada`.
Coluna "Canceladas" só aparece quando tem alguma.

| De | Para | Como | Quem |
|---|---|---|---|
| (criação) | a_fazer | criar na lista | você, agente, chefe |
| (criação) | em_andamento | criar e despachar | você, chefe |
| a_fazer | em_andamento | despachar, ou `task_next` | você, chefe, pane ociosa |
| em_andamento | a_fazer | pane morreu / devolveu / você arrastou de volta (mata a pane) | engine, agente, você |
| em_andamento | em_revisão | agente pediu revisão, ou PR aberto com CI verde | agente, engine |
| em_revisão | em_andamento | revisão pediu mudança, CI quebrou | engine (leva o feedback à pane dona) |
| em_revisão | concluída | agente ou você marcou; PR mergeado; contrato de PR satisfeito | agente, você, engine |
| qualquer | cancelada | você | você |
| cancelada | a_fazer | você | você |

Arraste: permitido entre colunas, exceto PARA `em_andamento` (só despacho ou
`task_next` colocam ali). Todo movimento grava `{ator, de, para, motivo}`
no histórico. O engine pode recusar um movimento com motivo, por exemplo
"tem subitem aberto" ou "depende de WS-9, ainda aberta".

## 4. Criar

Dois caminhos, o mesmo contrato:
- Diálogo "Nova tarefa": título, descrição, quem vai rodar, prioridade,
  worktree, permissão, depende de. Botões "Criar na lista" (⌘Enter) e
  "Criar e despachar" (⌘⇧Enter).
- Ferramenta MCP `task_create` com os mesmos campos. O histórico diz
  "criada pela aria".

## 5. Começar: despacho e `task_next`

**Despacho** (você ou o chefe): escolhe a tarefa e um preset (harness,
modelo, esforço, permissão, worktree). O engine abre a pane, cria a
worktree pelo worktrunk, prende pane e tarefa, e manda a tarefa como
primeiro prompt (título, descrição, como_se_verifica, subitens existentes,
comentários, worktree).

**`task_next`** (pane ociosa, geralmente pelo loop): a pane pede "me dá a
próxima". O engine escolhe a primeira em `a_fazer` que: não tem dependência
aberta; não pede um harness diferente do da pane (ou pede "padrão");
ordenada por prioridade e depois por criação. A reserva é atômica dentro do
engine (uma transação: "marca em_andamento e dono = esta pane só se ainda
está em a_fazer e sem dono"). Se não afetou uma linha, tenta a próxima. Se
não há nada, responde "vazia" e a pane continua ociosa.

**Sem batimento do agente.** O engine é dono das panes; ele sabe quando a
pane morre, fecha ou o processo sai. Isso substitui o lease com validade
usado por outros quadros. Regra de devolução: pane morreu ou foi fechada
com tarefa em `em_andamento` → tarefa volta a `a_fazer`, tentativa fecha
como "devolvida", comentário do sistema "devolvida: a pane fechou". Se a
pane ficar "sem sinal" por mais de N minutos (N = 10, configurável), a
tarefa NÃO volta sozinha; o card mostra "sem sinal" e entra na caixa de
atenção; você decide.

**Despacho duplo é impossível**: uma tarefa tem no máximo um dono; despachar
uma tarefa que já tem dono é recusado com motivo.

## 6. Trabalhar, travar, reportar

- Frase de estado no card, calculada de fatos, nesta ordem de prioridade:
  `esperando sua permissão` > `sem sinal há X` > `CI quebrado, corrigindo` >
  `revisão pediu mudança` > `trabalhando · último commit há X` >
  `livre para puxar` > `travada: <motivo>` > `concluída`.
- Comentários do agente = relatório. O primeiro prompt pede: "crie os
  subitens antes de começar; comente a cada marco; ao terminar peça
  revisão; se travar, diga o tipo".
- Bloqueio com tipo: `dependência` (espera outra tarefa; volta a
  `a_fazer` sozinha quando a outra fecha), `falta_informação` (espera você;
  entra na caixa de atenção), `erro_externo` (rede, quota; o engine tenta
  de novo 2 vezes com espera, depois vira `falta_informação`).
- Feedback de CI e revisão: quando o PR da tarefa tem CI vermelho, revisão
  pedindo mudança ou conflito, o engine entrega à pane dona uma mensagem com
  o resumo e o fim do log, uma vez por assinatura (mesmo erro não repete),
  no máximo 3 vezes por tarefa, rearmando quando o CI ficar verde, e nunca
  por cima de um diálogo de permissão ou de um rascunho humano na pane.

## 7. Revisar e concluir

- `em_revisão` mostra um de dois selos, calculado: **VEZ DO SISTEMA**
  quando o CI está vermelho ou a revisão pediu mudança e a pane dona está
  viva corrigindo; **SUA VEZ** quando o CI está verde e o PR espera uma
  pessoa, ou quando não há PR e o agente pediu revisão.
- Revisão por agente (PENDENTE Q3, assumi "depois da v1"): quando ligada,
  entrar em `em_revisão` abre uma segunda pane como revisora, que aprova
  ou devolve com motivo.
- Concluir: o agente chama `task_complete` com resumo, ou você arrasta.
  Com contrato de PR ligado (PENDENTE Q4), concluir exige PR mergeado ou
  checks obrigatórios verdes no commit atual.
- Ao concluir: tentativa fecha, worktree é limpa pelo worktrunk se a árvore
  está limpa e a branch está no remoto, senão fica e o card avisa.

## 8. Caixa de atenção (por workspace)

Lista com: tarefas `falta_informação`, panes `sem sinal`, permissões
pendentes, CI quebrado sem pane viva, movimentos recusados. Cada item tem
"ir para a pane". O cabeçalho do quadro mostra "N precisam de você".

## 9. Loop por pane

`{prompt, intervalo, máximo_de_rodadas, parar_quando}`. A cada intervalo o
engine manda o prompt à pane se ela estiver ociosa; se estiver no meio de
um turno, espera. Uso típico: prompt "puxe a próxima tarefa com task_next;
se vier vazia, responda 'nada a fazer'". Parar quando: rodadas esgotaram,
`task_next` devolveu vazia 3 vezes seguidas, ou você desligou.
Máximo de rodadas: PENDENTE Q1 (assumi 10, configurável, 0 = sem teto).

## 10. Agente-chefe do workspace

Uma pane com papel "chefe": recebe as ferramentas de tarefa completas
(criar, despachar, listar, comentar, desbloquear) e um prompt de papel:
"coordene, não implemente; crie tarefas pequenas e autossuficientes, com
como_se_verifica; nunca dependa da memória de outra tarefa; despache só
quando não houver pane livre para puxar". Ele não move tarefas para
`concluída`; isso é do executor ou seu.

## 11. Ferramentas MCP (nomes finais)

`task_create`, `task_next`, `task_show`, `task_list` (chefe e você),
`task_update` (título/descrição/prioridade/depende_de), `task_comment`,
`task_subitem_add`, `task_subitem_done`, `task_block {tipo, motivo}`,
`task_release` (devolver), `task_request_review`, `task_complete {resumo}`,
`task_dispatch` (só chefe), `task_unblock` (só chefe), `loop_set`.
Uma pane só pode mexer na tarefa que é dela, exceto o chefe.

## 12. Eventos para os clientes (Mac, Linux/Windows, iPhone)

`task.created`, `task.moved {ator, de, para, motivo}`, `task.claimed {pane}`,
`task.released {motivo}`, `task.commented`, `task.subitem_changed`,
`task.blocked {tipo}`, `task.phrase_changed {frase}`, `task.attention
{n}`, `loop.tick`, `loop.stopped {motivo}`. O quadro do cliente é uma vista
desses eventos; ele não calcula estado.

## 13. Fora da v1
Quadro entre workspaces; prioridade automática; tarefas recorrentes por
evento; sub-subitens; contrato de PR (se Q4 = depois); revisão por agente
(se Q3 = depois).

## Decididas pelo Caio (2026-09-10)
1. Loop: teto configurável (padrão 10, 0 = sem teto).
2. `como_se_verifica`: na v1, opcional, texto lido pelo agente; nada automático.
3. Revisão por agente: depois da v1.
4. Contrato de PR: depois da v1.

## Revisão aplicada (2026-09-10, achados R07, R08, R09, R13, R21, R22, R23)

**R07 · `task_next` é chamado DENTRO do turno do loop.** A ociosidade só autoriza o disparo do loop (o engine só manda o prompt do loop com a pane `ociosa`). A ferramenta `task_next`, chamada pelo agente já em turno, é aceita se: a chamada vem da `binding` da pane; a pane não tem outra tarefa ativa (uma tarefa por pane, no máximo); a pane não está fechando nem trocando de harness. A resposta devolve a tarefa; o engine não injeta outro prompt.

**R08 · como a tarefa puxada chega à worktree certa.** A sessão em que o agente esperava (a do loop) tem o cwd antigo e não vale nada de contexto. Regra: `task_next` reserva a tarefa, prepara a worktree pelo worktrunk e aplica o preset da tarefa (harness, modelo, esforço, permissão); a chamada MCP responde `{task, willRestartSession: true}`; o turno atual termina; o engine encerra a sessão de espera e abre uma sessão nova NA MESMA PANE, no cwd da worktree, com a tarefa como primeiro prompt. Só então a tarefa publica "em andamento · tentativa n". Se o preset pede harness diferente do da pane, é troca de harness (contrato 01) e não pacote de continuação (não há conversa a preservar). Nunca dois executores.

**R09 · repetição segura e tentativa atual.** Toda alteração feita pelo executor (`comment`, `subitem`, `block`, `request_review`, `complete`, `release`) carrega `{paneId, sessionGeneration, attempt}` e é recusada com `stale_attempt` se não for a tentativa ativa. Despacho grava as etapas (reservou, worktree criada, pane aberta, prompt aceito) e, após queda, retoma da última etapa ou desfaz (compensação) em vez de duplicar. Devolução de tarefa por "pane morreu" só depois de o engine reconciliar com o ptyd (contrato 07): pane viva no ptyd = não devolve.

**R13 · uma tabela só, por operação.** Correção do §3: CI vermelho ou revisão pedindo mudança NÃO tira a tarefa de `em_revisão`; ela fica em `em_revisão` com selo VEZ DO SISTEMA e o feedback vai para a pane dona (§6). Volta a `em_andamento` só por `task_release`, por arraste seu, ou se a pane dona morrer. Bloqueio é um CAMPO (`bloqueio {kind, reason}`), não um estado: tarefa bloqueada continua em `a_fazer` (ou onde estiver), não é puxável, e o card mostra "travada: …" com prioridade sobre "livre para puxar". Arraste permitido só pelas linhas da tabela do §3; arrastar `a_fazer → concluída` é recusado (`transition_refused`). Cancelar uma tarefa `em_andamento` cancela o turno da pane e fecha a tentativa antes de mover.

**R21 · fila de prompts durável e rascunho humano.** Toda fonte de prompt (você, loop, feedback de CI, chefe, pacote de continuação) entra numa fila durável por pane com origem, prioridade (humano > feedback > loop) e `promptId`. O cliente manda `pane.draft {active: true|false}` com validade de 10 s enquanto você digita; o engine não injeta prompt automático com rascunho ativo nem com diálogo de permissão aberto. Ticks do loop não acumulam durante ocupação ou parada do engine; a rodada conta no despacho efetivo; contadores e assinaturas de feedback são persistidos; as "3 vezes vazia" zeram quando uma tarefa é puxada.

**R22 · fim de tentativa e worktree.** Toda tentativa fecha com resultado (`concluída | devolvida | falhou | cancelada`) e desvincula a pane. Tarefa em `em_revisão` cuja pane morreu mantém o card, marca "sem executor" e aceita novo despacho ou correção por outra pane (tentativa n+1). Limpeza de worktree na v1 é MANUAL (botão "limpar worktree"), com verificação: nenhum processo usando, árvore limpa, e HEAD alcançável de uma branch fora da worktree; senão recusa com motivo. Branches nunca são apagadas automaticamente.

**R23 · dependências e edição concorrente.** Dependência só dentro do mesmo workspace; sem autorreferência e sem ciclo (criação recusada). Só `concluída` satisfaz; dependência `cancelada` deixa a dependente "travada: dependência cancelada" pedindo decisão. Mudar dependência de tarefa `em_andamento` bloqueia a tarefa até você confirmar. `task_update` exige `version` atual (conflito = `stale_version`). "Dono planejado" (quem você quer que faça) é campo separado de "executor ativo" (quem reservou): o planejado não impede pull por outra pane do mesmo harness, só ordena a preferência.
