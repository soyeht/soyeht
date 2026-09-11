# Plano de fatias · do zero até o produto na sua mão

Cada fatia é vertical (atravessa engine, contrato e pelo menos um cliente),
tem aceites automatizados e só fecha com eles verdes. Ordem pensada para
provar primeiro o que é base e deixar o experimento de estado por último.
Data: 2026-09-10.

| # | Fatia | O que entra | Contratos | O que prova |
|---|---|---|---|---|
| 1 | **Base** | engine + ptyd sob launchd (perfil Dev), esquema do protocolo com tipos gerados, banco, segmentos de tela, uma pane de shell, CLI, app Mac mostrando, app iPhone pareando e lendo | 06, 07, 08 | processo separado, protocolo único, reconexão, persistência, atualização sem matar processo |
| 2 | **Agente pelo ACP** | pane de agente com o nosso chat: abrir harness ACP v1, prompt, fila, permissão (Mac e iPhone respondem), cancelar, login; harness sob o ptyd sobrevivendo a reinício do engine; seletor harness · modelo · esforço; trocar modelo/esforço/permissão no meio; botão yolo; sondagem por versão | 04, 02, 05 (parte ACP), 07 | o coração do produto funciona e sobrevive a atualização |
| 3 | **Tarefas** | lista por workspace, quadro e vista lista, criar (diálogo e MCP), despachar, `task_next` com reserva e devolução, subitens, comentários, frase de estado, selos "sua vez/vez do sistema", bloqueio tipado, loop por pane, agente-chefe, feedback de CI para a pane dona, caixa de atenção, worktree por tarefa (worktrunk) | 03, 08 | o fluxo de trabalho com vários agentes |
| 4 | **Troca de harness** | prompt de continuação do arquivo único, 6 mensagens, endereço do transcript, troca e volta, fila congelada, falha com restauração | 01 | multi-harness de verdade |
| 5 | **Terminal cru e o experimento** | gravação de quadros + rótulos de gancho + export; ponto de encaixe do detector; regex do app atual como linha de base; estados "sem sinal" e "não sei" | 05 | dá para medir e decidir o detector com dados |
| 6 | **iPhone completo** | quadro de tarefas, despachar, yolo, trocar modelo, push (APNs pelo engine), Live Activity, reconexão em todas as telas | 06, 03 | supervisão de fora de casa |
| 7 | **Linux e Windows** | app TypeScript com o mesmo cliente do protocolo, terminal e pane de agente; named pipe no Windows | 08 | o produto fora do Mac |
| 8 | **Produção** | assinatura e notarização, TestFlight, atualizador do engine que não mata o ptyd, migração dos dados do app atual para o novo (workspaces, dispositivos pareados), perfil de produção, changelog | 07 | você troca o app de hoje pelo novo |

Depois da 8, entram as propostas em aberto que você quis conversar depois:
contexto compartilhado do workspace, chefe reagindo a eventos, revisão por
agente, contrato de PR, WikiSkill.

Regra de tamanho: cada fatia é um objetivo autossuficiente como
`00-fatia-1-objetivo.md`, executado até o fim pelo Codex em modo objetivo,
revisado por mim contra os contratos, e aprovado por você olhando o produto
no perfil Dev.
