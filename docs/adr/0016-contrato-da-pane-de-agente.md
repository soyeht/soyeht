# ADR 0016 · Contrato da pane de agente

Status: aceita. Escopo: fatia 2.

Os clientes continuam falando somente o protocolo do engine. A pane ganha
um campo opcional `agent`, com harness, versão, impressão digital, sessão,
opções confirmadas, operação pendente, fila, rascunho e modo de permissão.
Panes de terminal já persistidas continuam válidas sem esse campo.

Cada prompt aceito recebe `promptId`; a origem é `human` ou `automatic`.
O limite é 64 KiB de texto e 64 prompts aguardando. `pane.draft` informa a
presença de texto humano ainda não enviado, sem transmitir esse texto.
O estado agregado fica no snapshot; `pane.queued` também informa o rascunho.

`pane.update` é uma união fechada dos oito tipos do contrato 08, com
identidade de mensagem, geração e sequência. Pensamento, ferramenta, plano,
uso e terminal são distintos. O transcript guarda mensagens normalizadas,
completa os pedaços na mesma identidade e limita páginas a 200 mensagens
ou 512 KiB. Mensagens maiores são divididas em registros de até 256 KiB.

Pedidos de permissão e elicitação têm identidades próprias, geração, turno
e prazo. Uma resposta atrasada retorna o resultado que já venceu; nunca
produz outra resposta nativa. `by` distingue Mac, iPhone, regra e sistema.
Yolo não responde elicitações. O modo do engine usa `ask`, `accept_edits` e
`allow_all`; o modo nativo efetivamente confirmado é guardado separadamente.
Opções de configuração aceitam valores de seleção ou booleanos.

A sessão v1 termina pela resposta de `session/prompt`. A implementação
segue o [esquema oficial v1](https://github.com/agentclientprotocol/agent-client-protocol/blob/2821a8b51dca2aeb3d48b73c9214a6c79968c975/schema/v1/schema.json).
A fatia implementa v1; uma versão não implementada é recusada explicitamente,
sem interpretar mensagens v2 como v1. A adaptação v2 precisa de uma entrega
própria quando houver o harness real previsto no objetivo.

O contrato 02 prevê pacote ao reiniciar uma configuração não suportada,
mas o objetivo reserva esse pacote para a fatia 4. Nesta fatia, configuração
não suportada falha com o valor anterior preservado. Perda comprovada de
processo tenta `session/load`; perda comprovada da sessão abre outra, com
`pane.session_lost {recovery: new_session}` e aviso persistido. Não se promete
continuação de contexto que ainda não foi implementada. Erro de parâmetros
ou timeout não é prova de perda e não abre outra sessão.
