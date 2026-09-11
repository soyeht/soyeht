# 0003 — Canal do supervisor de processos

Estado: aceito para implementação, 2026-09-11.

## Contexto

O contrato 07 exige processos independentes do engine, registro em arquivo,
retenção limitada, confirmação de leitura e um único engine conectado.
Ele não fixa o formato do canal interno nem a unidade de sequência.

## Decisão

O canal usa socket Unix privado e quadros JSON com o mesmo limite de 1 MiB
do codec comum. A versão interna é independente da versão dos clientes.
Cada pedido recebe uma resposta. O engine busca a saída retida em lotes;
a leitura do PTY acontece em outra thread e não depende dessa busca.

`seq` é um offset de bytes, exclusivo: confirmar 100 confirma os bytes
anteriores a 100. Cada processo começa em zero. Lotes carregam o intervalo
e a próxima posição; uma busca anterior ao início retido devolve um gap
explícito. A saída retida nunca ultrapassa 8 MiB por processo.

O registro é substituído atomicamente e sincronizado antes de confirmar
spawn, close ou uma nova geração do engine. Uma chave de spawn fechada
permanece fechada e não cria outro processo. O identificador do supervisor
muda a cada partida; processos da partida anterior não são adotados por PID.

Só uma conexão de engine pode executar comandos. Uma geração maior invalida
a conexão anterior; uma menor é recusada. A mesma geração não toma uma
conexão ainda viva. Um lock de arquivo impede dois supervisores no perfil.

## Motivo

Offsets permitem transformar a retenção em segmentos sem inventar outra
numeração. A busca em lotes mantém a saída do processo independente de um
cliente lento. Identificadores de instância evitam confundir um PID reutilizado
com o processo de uma pane anterior.
