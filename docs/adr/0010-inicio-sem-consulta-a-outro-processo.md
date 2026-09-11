# ADR 0010 — Início sem consulta a outro processo

Status: aceita para a fatia 1.

Na instalação real sob o gerenciador de serviços, chamar o utilitário de
rede antes de abrir o socket deixou o engine esperando indefinidamente.
O engine passa a consultar diretamente as interfaces do sistema com
`getifaddrs`, sem iniciar outro processo.

No Mac, seleciona interfaces `utun` ativas com endereços do intervalo IPv4
100.64.0.0/10 ou do prefixo IPv6 fd7a:115c:a1e0::/48. Interfaces físicas,
endereços da LAN e endereços coringa são excluídos. O listener local continua
disponível sem a tailnet. Um teste cobre a exclusão desses endereços.

O instalador só declara o serviço pronto depois que seu socket aceita uma
conexão, com prazo de quinze segundos. Se o processo não ficar pronto,
devolve erro e indica o log privado; não mexe em outros serviços.
