# 0006 — Estado do shell sem inferência de prontidão

Estado: leitura conservadora para a fatia 1, 2026-09-11.

O objetivo enumera estados de processo para a primeira fatia. O contrato 05
é mais restritivo: cobertura parcial de terminal cru aparece como `não_sei`,
nunca como `ociosa`; prontidão não pode ser inventada antes de um sinal real.
A fatia não inclui ganchos, reconhecimento da tela nem o experimento adiado.

O engine usa `iniciando` durante a criação, `recuperando` enquanto reconcilia
o supervisor e `morta` após confirmar a saída. Um shell vivo sem sinal
verificável de prontidão fica em `não_sei`, com source `none`, confiança zero
e a frase "Shell ativo". Silêncio de saída não é usado para declarar ociosidade.

Isso preserva o contrato 05 e impede que uma indicação visual vire permissão
falsa para despachar trabalho. O terminal continua aceitando entrada humana.
Os estados de agentes por protocolo pertencem à fatia seguinte. A divergência
entre a enumeração resumida do objetivo e esta leitura fica no relatório final.
