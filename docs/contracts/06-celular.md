# Contrato 06 · iPhone

Estado: FECHADO em 2026-09-10. Decisão do Caio: "seguir o que tem hoje,
está confirmado que funciona". Prova de como é hoje em
(análise interna, fora do repositório) ("Como o iPhone chega no Mac HOJE").

## 1. Regras (iguais às de hoje, transportadas para o engine e o app novos)
1. **Rede**: o engine escuta na porta do Soyeht nos endereços do Tailscale
   (v4 e v6) e no loopback. Na LAN só enquanto a janela "Adicionar iPhone"
   está aberta. Nunca `0.0.0.0`. Anúncio Bonjour na LAN e no tailnet.
2. **Transporte**: HTTP e WebSocket em texto puro dentro do tailnet; a
   criptografia é do Tailscale. O app iOS novo carrega a mesma exceção de
   ATS (`NSAllowsArbitraryLoads`) com o mesmo comentário de justificativa.
   Evolução opcional, sem mudar o resto: HTTPS/WSS via `tailscale serve`.
3. **Pareamento**: QR (ou URI) com endereço + código de uso único; o app
   abre a conexão, responde um desafio HMAC, o Mac pede consentimento ao
   dono; recusas com motivo (expirado, já usado, desafio inválido, negado,
   revogado). Uma vez por aparelho; revogável nas preferências.
4. **Protocolo**: o MESMO contrato de eventos e pedidos dos outros clientes
   (contratos 03 §12 e 04 §9); o iPhone é uma vista da sessão do Mac.
5. **Reconexão**: ao voltar do fundo, o app pede o estado inteiro (panes,
   tarefas, pedidos de permissão pendentes) antes de mostrar qualquer
   coisa; nunca só reabrir o socket (lição da análise interna).
6. **Push**: o engine no Mac manda para a APNs direto (como hoje), para:
   pedido de permissão, tarefa "sua vez", "sem sinal", pane morreu. Live
   Activity para "agente rodando" (já existe para deploy; reaproveitar).
7. **O que o iPhone faz na v1**: ver workspaces, panes e estado; ler a
   pane (agente e terminal cru); responder permissão; mandar texto; quadro
   de tarefas (ver, arrastar, despachar); botão yolo; trocar modelo/esforço.

## 2. Fora da v1
Relay na nuvem; conexão sem Tailscale; malha própria (10.44/16) para o
iPhone; digitar ao vivo no terminal cru pelo iPhone.

## Revisão aplicada (2026-09-10, achado R10)
O caminho QR → desafio → consentimento → `deviceToken` está definido no contrato 08 §2.2; push e Live Activity em 08 §2.3; revogação invalida token, conexões e push. Digitar no terminal cru pelo iPhone fica fora da v1 (matriz de papéis, 08 §2.4).
