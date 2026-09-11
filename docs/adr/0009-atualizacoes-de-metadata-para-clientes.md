# ADR 0009 — Atualizações de metadados para clientes

Status: aceita para a fatia 1.

Redimensionar uma pane e mudar sua gravação alteram o snapshot. O recorte
do protocolo não nomeava um evento para publicar essas alterações. O engine
usa a extensão aditiva `pane.updated {pane}`, persistida na mesma transação.
Isso mantém os demais clientes atualizados sem pedir uma nova foto inteira.
`settings.changed {key, value}` faz o mesmo para as configurações.

O resultado final do snapshot inclui `focus {workspaceId?, paneId?}`. Esse
foco pertence à identidade do cliente autenticado. Reconectar o Mac restaura
seu foco sem aplicar o foco escolhido no telefone. Fechar a pane limpa as
referências a ela na mesma transação.

A seção `requests` inclui o tipo `DevicePairPending`, com nome e impressão
digital do aparelho, identidade do pedido e prazo. A prova, o código e o
token nunca aparecem nessa seção. Os novos campos e eventos estão no esquema
único e têm vetores para os três consumidores gerados.
