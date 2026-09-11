# ADR 0014 · Fila cheia e registro de operações

Status: aceita. Escopo: fatia 1.

Ao exceder 8 MiB, a conexão descarta a fila e tenta escrever uma única resposta
de erro JSON-RPC, com `id: null`, código numérico e `kind: slow_consumer`.
Em seguida fecha o transporte. O erro não espera espaço na fila que já encheu.
Os escritores de snapshot são liberados quando as partes pendentes são descartadas.

Se o transporte parou de aceitar bytes, não é possível garantir que o cliente
receba o motivo. A tentativa de escrita tem o mesmo prazo de cinco segundos das
demais escritas. O fechamento continua obrigatório. O cliente trata tanto o erro
quanto a queda como motivo para buscar snapshot e assinar eventos novamente.
Nenhuma continuidade é prometida para os bytes que ficaram na fila descartada.

Cada pedido recebe um identificador de operação criado pelo engine. O log JSON
contém esse identificador, a conexão, o método conhecido pelo esquema, o resultado,
o tipo de erro e a duração. Não contém parâmetros, IDs fornecidos pelo cliente,
texto de erro livre, teclas, desafios, códigos, tokens ou chaves. Um método que
não pertence ao esquema aparece como `unknown`, sem copiar o texto recebido.

O controle da fila verifica o limite de memória, o erro numérico antes do fim
do fluxo e a liberação dos bytes. O controle do registro verifica que texto
arbitrário no nome do método não entra no log.
