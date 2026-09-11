# ADR 0007 — Repetição da entrada de terminal

Status: aceita para a fatia 1.

A digitação muda um processo externo. Uma transação do banco não desfaz essa mudança. O engine grava primeiro uma intenção com a identidade do supervisor e do processo. A intenção guarda um resumo criptográfico do pedido, nunca os bytes digitados.

O supervisor guarda o resultado da escrita por identidade de operação até o engine confirmar que seu próprio recibo está durável. Repetir o pedido no mesmo supervisor devolve a quantidade original de bytes aceitos. Uma escrita parcial devolve sua quantidade; o cliente envia somente o restante com outra identidade.

Se o supervisor morrer entre a escrita e o recibo durável, o resultado fica desconhecido. O engine recusa repetir aquela operação em um shell substituto com `resync_required`. Essa leitura conservadora evita executar duas vezes um comando cujo resultado não pode mais ser consultado. O usuário pode enviar uma nova entrada depois de verificar a tela.

O supervisor aceita no máximo 1.024 recibos pendentes. Ao atingir esse limite, recusa novas escritas antes de tocar no terminal; as confirmações liberam espaço. Reiniciar apenas o engine preserva os recibos e o processo.
