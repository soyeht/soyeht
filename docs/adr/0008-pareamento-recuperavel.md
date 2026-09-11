# ADR 0008 — Pareamento recuperável

Status: aceita para a fatia 1.

O código de pareamento e os pedidos pendentes duram dez minutos. O banco
guarda a prova de conclusão e o resultado aprovado até esse prazo. Assim,
uma queda entre o consentimento no Mac e a resposta ao telefone não exige
aprovar o mesmo aparelho duas vezes. Para recuperar o resultado, o telefone
precisa apresentar novamente a prova vinculada à chave e ao desafio.

O registro permanente do aparelho contém a chave pública e o hash do token.
O recibo temporário de pareamento contém o token para permitir sua entrega
novamente. Esse recibo fica no banco privado do usuário, com arquivo 0600 e
diretório 0700, e é removido após o prazo. Isso não promete proteção contra
outro processo que já possua os poderes do mesmo usuário do sistema.

O snapshot inclui os pedidos já provados que ainda esperam consentimento.
Reconectar o app Mac não esconde um pedido que chegou durante sua ausência.
O código não abre a rede local: o transporte remoto usa um endereço da
tailnet. Sem esse endereço, o fluxo remoto depende de disponibilizar a
tailnet antes de iniciar o engine.

As gerações do engine e do supervisor são contadores separados. A primeira
muda ao reiniciar o engine; a segunda muda apenas ao reiniciar o supervisor.
O registro persistente do supervisor mantém seu formato 1, com o contador
adicional opcional ao ler arquivos anteriores. O protocolo entre os dois
processos passa à versão 2 para incluir recibos de entrada.
