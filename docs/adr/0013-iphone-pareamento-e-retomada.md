# ADR 0013 · iPhone, pareamento e retomada

Status: aceita. Escopo: fatia 1.

O app iPhone usa o mesmo pacote Swift e os mesmos tipos gerados do Mac.
O transporte remoto usa WebSocket; texto contém JSON e quadros binários
mantêm o cabeçalho comum. O parser do QR aceita somente o esquema do produto,
três campos únicos, porta válida e endereço literal de tailnet ou loopback.
A identidade do engine recebida na conexão precisa coincidir com a do QR.

O iPhone cria uma chave Ed25519. O desafio HMAC inclui o código, a chave pública,
o desafio e o nome mostrado no Mac. A conexão seguinte assina o nonce, o engine
e o token. Credencial e chave ficam no Keychain, acessíveis somente após o
aparelho ser desbloqueado e sem migração para outro dispositivo. Nada vai para
preferências, URL de conexão ou logs. O código só aparece no QR e no pedido de
pareamento. A câmera usa o leitor nativo; dispositivos sem esse leitor podem
abrir ou colar o link do QR.

A aprovação consome o código na mesma transação que cria a credencial. Uma
nova tentativa com esse código é recusada. A tentativa original conserva seu
recibo por até o prazo do pareamento, para recuperar uma resposta perdida.
O controle ficou vermelho antes da correção e verde depois; também verifica
que a recuperação devolve a mesma credencial. O Mac pode revogar o aparelho
na janela de pareamento, com confirmação.

Ao entrar no fundo, o iPhone fecha a conexão e remove a tela e a lista visíveis.
Ao voltar, faz handshake, snapshot completo e assinatura de eventos. Só mostra
a lista após concluir esse ciclo. Revogação fecha a conexão e uma credencial
recusada exige novo pareamento. O shell e o histórico continuam no Mac.

A tela do iPhone não aceita teclado, clipboard remoto nem abertura de links
emitida pelo terminal. Resize continua sendo um pedido permitido: o último
resize vence. Aplicar as dimensões recebidas no interpretador não gera outro
pedido, para impedir disputa entre os dois clientes.

A assinatura local é automática de desenvolvimento. O build do iPhone passou
com o certificado e o perfil de desenvolvimento existentes. A CI compila Mac e
iOS Simulator sem assinatura. Reboot físico, câmera real e troca para rede móvel
permanecem no roteiro manual do aceite 7; o teste do protocolo não substitui isso.
