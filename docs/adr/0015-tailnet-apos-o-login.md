# ADR 0015 · Tailnet depois do login

Status: aceita. Escopo: fatia 1.

No reboot, não há garantia de que a interface da tailnet apareça antes do
LaunchAgent do engine. Consultá-la apenas uma vez deixava o engine restrito ao
loopback até intervenção manual. Isso prejudicava a retomada pelo iPhone.

Na configuração padrão, o engine consulta as interfaces a cada dois segundos.
Uma mudança no conjunto de endereços provoca saída normal; o launchd, com
KeepAlive, inicia outra instância que abre os endereços atuais. O `ptyd` não é
reiniciado. O engine novo adota os shells e os clientes refazem snapshot e
assinatura, usando o mesmo mecanismo dos aceites de reinício.

Essa escolha usa o caminho de recuperação já testado. Não cria outro dono
para os sockets nem muda o transporte. Uma flutuação de rede pode causar um
reinício adicional do engine; o log registra `engine.network_changed` sem
copiar endereços. Erro de consulta não conta como desaparecimento da interface.

Endereços explícitos por `--listen` mantêm a configuração fixa, usada nos testes
isolados. O instalador Dev usa a configuração automática. As verificações de
interface e faixa continuam recusando interfaces físicas, LAN e wildcard.
O teste cobre aparecimento tardio, remoção, ordem diferente e erro transitório,
sem alterar nenhuma interface da máquina.
