# Fatia 1 · roteiro manual no perfil novo

Este roteiro não foi executado em um Mac reiniciado nem em um iPhone na rede
móvel durante a implementação. Os aceites 1 a 6 têm execução automática própria.
Aqui ficam o aceite 7 e a conferência dos dois aplicativos em aparelhos reais.

## Preparar

1. Siga [a instalação Dev](dev-local.md), [o app Mac](apps-mac.md) e
   [o app iPhone](apps-iphone.md). Use as mesmas fontes para engine e aplicativos.
2. Confira `soyeht --profile dev status`. Os serviços devem se chamar
   `com.soyeht.next.dev.engine` e `com.soyeht.next.dev.ptyd`. A porta é 8291.
   Confira o caminho do executável de cada serviço com `launchctl print`.
3. No Mac novo, crie uma pane de shell em uma pasta de teste sua. Anote o
   workspace, nome da pane, diretório, `paneId`, `pid` e `generation` retornados
   por `soyeht --profile dev pane list`. Não use uma pane de trabalho real.
4. Digite `printf 'ACEITE_FATIA_1\n'; pwd` nessa pane e espere a saída aparecer.
   Anote a hora. Não coloque credenciais nos comandos ou nas evidências.

## Parear e ler na rede móvel

1. Conecte Mac e iPhone à mesma tailnet. No app Mac novo, escolha
   “Adicionar iPhone”. No app iPhone novo, escolha “Ler QR do Mac”.
2. Antes de permitir, confira que o iPhone mostra espera e não recebe a lista
   de panes. Compare o nome e a impressão digital completa nos dois aparelhos.
3. Permita no Mac. Abra a pane no iPhone. A saída `ACEITE_FATIA_1` e o diretório
   devem aparecer. O iPhone é somente leitura nesta fatia.
4. Desligue o Wi-Fi do iPhone, mantendo dados móveis e Tailscale. Volte ao app.
   Espere a sincronização. Digite `printf 'REDE_MOVEL_OK\n'` no Mac e confira
   a saída no iPhone. Registre horário e resultado, sem fotografar QR ou tokens.
5. Mande o app iPhone ao fundo por um minuto. Crie outra pane no Mac. Volte ao
   iPhone: a lista deve refletir as panes atuais, sem mostrar estado antigo
   como conectado. Feche a pane extra no Mac e confira a remoção no iPhone.
6. Feche e reabra o app iPhone. O pareamento deve continuar salvo no Keychain;
   nenhum QR novo é necessário enquanto a credencial não foi revogada.

## Reiniciar o Mac

1. Salve o trabalho pessoal antes de reiniciar o computador pelo menu do sistema.
   O reboot é uma ação manual deste roteiro; nenhum script de teste o dispara.
2. Após login, aguarde os dois serviços novos. Abra o app Mac novo e o iPhone.
3. Confira o mesmo workspace, pane e diretório. O shell deve ter nova geração
   e a indicação “shell novo”. O processo anterior não continua após um reboot;
   um PID numérico eventualmente reutilizado não prova continuidade.
4. A saída gravada `ACEITE_FATIA_1` deve aparecer no replay, seguida da saída
   do shell novo. Os últimos segundos anteriores ao reboot podem faltar.
   Uma lacuna declarada deve aparecer como tal, nunca como continuidade falsa.
5. Digite `printf 'DEPOIS_DO_REBOOT\n'; pwd` no Mac. Confira a saída no Mac e
   no iPhone e o diretório esperado. Registre os IDs e a geração nova.
6. Feche a pane de teste pelo Mac. Reabra os clientes e confirme que ela não
   voltou. Não exclua dados ou serviços de outros perfis durante a limpeza.

## Revogar

No Mac, abra a janela de pareamento, encontre o aparelho e escolha “Revogar”.
Confirme. O iPhone deve perder acesso, apagar a tela visível e pedir novo
pareamento. Reabrir o app não pode restaurar uma credencial revogada.

## Registrar o resultado

Para cada etapa, anote data, versão do sistema, commit, resultado e evidência
sem segredos. Marque separadamente: leitura por rede móvel, retorno do fundo,
credencial após reabrir, restauração após reboot e revogação. Se uma falhar,
registre os IDs de operação dos logs do perfil novo e abra um ajuste; não marque
o aceite manual como aprovado com base apenas no build ou no Simulator.
