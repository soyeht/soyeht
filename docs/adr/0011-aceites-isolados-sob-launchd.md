# ADR 0011 — Aceites isolados sob launchd

Status: aceita para a fatia 1.

Os seis aceites usam processos reais e os mesmos modelos de serviço da
instalação Dev. Cada execução recebe nomes únicos dentro de
`com.soyeht.next.dev.e2e.*`, uma pasta temporária curta e cópias dos binários.
Assim, rodar o teste não substitui o perfil Dev que já está instalado.

Os serviços usam o domínio gráfico do usuário. Se o runner não tiver esse
domínio, usam o domínio de usuário. O teste nunca usa o domínio de sistema.
Ao terminar, encerra as panes pelo supervisor e descarrega apenas os dois
serviços que criou.

O shell do teste usa `ZDOTDIR` apontando para arquivos próprios, sem histórico
pessoal. O teste espera o prompt controlado antes de enviar comandos; um PID
existente não prova que a inicialização do shell acabou. O supervisor passa
essa variável quando ela existe no seu ambiente. A instalação normal mantém
as configurações do usuário.

O aceite de reinício envia SIGKILL ao engine e observa o reinício automático
pelo serviço. O aceite de estouro descarrega apenas o engine, espera o shell
terminar de produzir mais de 8 MiB e volta a carregar o engine. O mesmo
supervisor permanece vivo. O replay por offset precisa declarar a lacuna.

O relatório registra cada aceite separadamente e também as falhas. A CI
publica o conteúdo nos logs e no resumo da execução. A conexão real do iPhone com Wi-Fi
desligado e o reboot do Mac continuam no roteiro manual.

O runner executou os seis aceites em 6 segundos. A conta atingiu a cota
de artefatos do GitHub; o relatório JSON fica nos logs e no resumo da
execução. A aprovação continua dependendo do resultado dos testes.
