# ADR 0012 · Cliente nativo e tela do terminal

Status: aceita. Escopo: fatia 1, app Mac e base compartilhada com o iPhone.

O engine continua dono do shell, dos dados e das permissões. Os clientes
precisam interpretar sequências de terminal, sem abrir processos próprios.
Usamos somente a `TerminalView` de SwiftTerm 1.20.0, fixada no lockfile.
A licença MIT completa acompanha o bundle, copiada do checkout da dependência
antes da assinatura. O projeto permanece Apache-2.0. O plugin da dependência
foi lido: consulta sua própria revisão e escreve metadados na saída do build.

O pacote compartilhado usa os tipos gerados do protocolo. O socket Mac usa
operações assíncronas; snapshot só substitui a lista após todas as partes.
Cada inscrição possui referência binária, geração e offset. Uma lacuna aparece
na interface; bytes fora de ordem forçam reconexão. O cliente refaz snapshot,
assinatura de eventos e tail após reconectar. O reset limpa o interpretador;
uma sequência de escape pode continuar no próximo quadro.

Entrada de teclado é serializada, limitada a 1 MiB e dividida em pedidos de
16 KiB. Uma repetição após desconexão conserva a chave de idempotência.
Respostas de consultas emitidas pela interpretação do replay não viram entrada
nova. Comandos de clipboard e abertura automática de links vindos da saída
não são executados. Copiar e colar por ação do usuário continua disponível.

A Tela 1 fornece cores, relevo, raios e proporções. A tipografia usa fontes de
sistema arredondada e monoespaçada, com escala nativa e acessibilidade. A fatia
mostra apenas workspaces, shells e pareamento, sem controles de funções futuras.

O primeiro teste do cliente mostrou que a adoção do shell alterava PID e
geração no banco, mas só publicava os campos de estado. `pane.updated` passa
a acompanhar `pane.state_changed` na mesma transação. Assim o snapshot e a
lista atualizada por eventos representam a mesma pane.

O build Debug aceita `SOYEHT_TEST_SOCKET` apenas para `engine.sock` em uma
pasta descartável `sn-*` diretamente sob o diretório temporário do sistema.
O teste verifica a recusa de perfis instalados e de caminhos fora dessa pasta.
O build Release não lê essa variável. Isso permite testar a janela com um shell
controlado sem alterar arquivos de inicialização do usuário.
