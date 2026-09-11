# Fatia 1 · entrega e verificação

Data: 2026-09-11. Perfil: `Soyeht.next/dev`. Licença: Apache-2.0.

A entrega implementa protocolo, supervisor, armazenamento, engine, CLI e apps
novos para Mac e iPhone. Os seis aceites automatizados passaram. O aceite manual
está [descrito em roteiro](../roteiro-manual-fatia-1.md); reboot físico, câmera
real e leitura pela rede móvel ainda precisam ser executados em aparelhos reais.
Nenhuma tag ou release foi criada.

## O que foi entregue

O esquema em `contracts/protocol.json` é a fonte dos tipos Rust, Swift e
TypeScript. São 26 métodos e 22 eventos no recorte, com vetores comuns, limites
binários e erros numéricos. A geração sem diferença é obrigatória na CI.

O `ptyd` mantém os shells separados do engine, com criação idempotente,
geração exclusiva, retenção de 8 MiB e confirmação por sequência. O engine usa
SQLite WAL com `synchronous=FULL`, segmentos de 512 KiB, retenção de 32 MiB por
pane e replay por geração e offset. Lacunas são declaradas.

A CLI abre, lista e fecha panes e consulta estado e pareamento. O app Mac
mostra o terminal, envia teclado e resize e permite parear ou revogar o iPhone.
O iPhone lê a pane por WebSocket, guarda sua identidade no Keychain e busca
snapshot completo ao voltar do fundo. Todos usam o protocolo do engine.

A fila cheia tenta comunicar `slow_consumer` e fecha a conexão. Os logs de
pedidos têm identificador de operação, método conhecido, resultado e duração,
sem copiar parâmetros ou credenciais. Se a tailnet aparecer depois do login,
o engine detecta a mudança e o launchd o reinicia; o supervisor conserva os shells.

## Evidência automatizada

| Aceite | Resultado | Verificação |
| --- | --- | --- |
| 1 · reabrir cliente | verde | Mesma pane e processo; replay preservado. |
| 2 · matar engine | verde | SIGKILL e reinício real pelo launchd; mesmo PID do shell. |
| 3 · snapshot e eventos | verde | Mudanças durante a desconexão; cursor sem perda ou duplicação. |
| 4 · fechar pane | verde | Todos os clientes removem; reinício não ressuscita. |
| 5 · repetir criação | verde | Uma pane e um processo para a mesma chave. |
| 6 · estourar retenção | verde | Mais de 8 MiB com engine parado; replay declara `stream.gap`. |

`scripts/e2e.py` executa os seis sob serviços temporários próprios. A execução
local de fechamento passou em menos de dez segundos. O relatório JSON fica em
`.cache/e2e.json`; na CI, aparece nos logs e no resumo da execução.

Fmt, clippy com avisos como erros, testes Rust, testes do gerador e da guarda,
vetores Swift/TypeScript e builds Mac/iOS passaram. Os sete testes nativos
Swift incluem shell real, entrada, reconexão, pareamento HMAC com consentimento,
assinatura de conexão, retorno com snapshot novo e revogação. O teste de rede
cobre aparecimento tardio, remoção, ordem de endereços e erro de consulta.

A CI do [PR 8](https://github.com/soyeht/soyeht/pull/8) passou com todos os
componentes: [execução](https://github.com/soyeht/soyeht/actions/runs/34570456920).
Foram 4 min 14 s entre criação e encerramento da execução, incluindo a espera
inicial do runner; o job principal levou 4 min 6 s. Isso é uma medição, não uma
garantia sobre filas futuras. O PR de fechamento executa a mesma matriz sobre
as correções finais.

## Aplicativos e verificações manuais feitas

O app Mac novo foi aberto contra um engine descartável. A entrada de teclado
produziu saída real no shell e apareceu no terminal. O teste encontrou e corrigiu
a transferência de foco para o terminal. A janela e as dimensões foram conferidas.

Os builds locais Mac e iPhone passaram com assinatura automática de
desenvolvimento do time configurado. A CI compila os dois sem assinatura.
O iPhone abriu no Simulator assinado e consultou o Keychain sem erro. O build
sem assinatura não fornece a identidade necessária para executar esse acesso.

O roteiro visual completo de pareamento no Simulator não foi concluído: a
automação de toque não conseguiu finalizar o diálogo do sistema. O pareamento,
a leitura e a revogação foram exercitados pelos clientes Swift contra o engine
real. Gravação e retomada da credencial pela interface em aparelho, câmera,
reboot e rede móvel continuam pendentes do aceite manual. Não estão marcados
como aprovados por causa do build ou dos testes do protocolo.

## Rodar do zero em um Mac limpo

1. Instale as ferramentas fixadas em [toolchains](../toolchains.md): Xcode,
   Rust, Python e Node. Configure a conta de desenvolvimento e a tailnet.
2. Clone o repositório privado e crie sua worktree:

   ```sh
   git clone git@github.com:soyeht/soyeht.git ~/Documents/soyeht
   cd ~/Documents/soyeht
   git worktree add ../soyeht-dev -b dev/local origin/main
   cd ../soyeht-dev
   ```

3. Execute os comandos de verificação do [README](../../README.md). Eles
   compilam os binários antes de executar os testes nativos e os aceites.
4. Instale o perfil novo e confira o engine:

   ```sh
   python3 scripts/install_dev.py
   engine/target/debug/soyeht --profile dev status
   engine/target/debug/soyeht --profile dev pane create --shell --cwd "$PWD"
   ```

5. Compile o app Mac com assinatura de desenvolvimento e abra o resultado:

   ```sh
   SOYEHT_DEVELOPMENT_TEAM=SEU_TIME bash scripts/check_mac.sh --sign
   open '.cache/mac-derived/Build/Products/Debug/Soyeht Dev.app'
   ```

6. Abra `apps/ios/SoyehtPhone.xcodeproj`, escolha o esquema `SoyehtPhone`,
   o iPhone e o time de desenvolvimento. Execute. Siga o
   [pareamento](../apps-iphone.md) e o [roteiro manual](../roteiro-manual-fatia-1.md).

O instalador recusa substituir um serviço ainda carregado. Para atualizar
somente o engine preservando shells, siga [Dev local](../dev-local.md).
Não misture binários de commits diferentes na validação. Para identificar o
build local, registre `git rev-parse HEAD` junto ao resultado dos testes.

## Decisões e limites conhecidos

- ADRs 0001–0002: entregas por PR e formato do protocolo/identidades.
- ADRs 0003–0004: supervisor, sequência e transações com segmentos.
- ADR 0005: identidade do merge produzido pelo serviço do GitHub.
- ADRs 0006–0007: estado do shell cru e repetição segura de entrada.
- ADRs 0008–0010: pareamento recuperável, eventos completos e início sem
  depender de um comando externo.
- ADRs 0011–0013: aceites isolados e clientes nativos.
- ADRs 0014–0015: motivo de fechamento, logs de operação e tailnet após login.

A proteção técnica da main depende do plano do repositório privado:
[issue 9](https://github.com/soyeht/soyeht/issues/9). A regra de processo continua
sendo PR com checks verdes, sem push direto. A autorização de execução permitiu
merge autônomo com revisão posterior. Isso não equivale a proteção técnica.

Os commits locais usam a identidade Git configurada de Caio Salgado, tanto
no autor quanto no committer. O squash do GitHub registra o committer do serviço
e o autor da conta autenticada. A API recusou o e-mail local como opção de autor;
a conta não foi alterada. Essa diferença está registrada no ADR 0005 e não foi
ocultada com reescrita da main.

A rotação dos logs dos serviços ainda precisa de teto de disco:
[issue 10](https://github.com/soyeht/soyeht/issues/10). A retenção dos segmentos de
terminal já tem seu limite. A cota de artefatos da conta impediu upload do JSON;
o resultado permanece nos logs e no resumo da CI, sem dispensar qualquer teste.

Uma mudança da tailnet causa reconexão dos clientes. Um cliente que não lê
pode deixar de receber até o erro de fila cheia; o engine fecha mesmo assim.
A durabilidade depende de o armazenamento cumprir a confirmação de escrita.
Reboot perde o processo de shell; restaura estado e saída gravada com geração nova.

O perfil Dev isola nomes e dados. Não cria uma fronteira extra de sistema
operacional para os shells do usuário. Pane de agente, tarefas, loops, ACP,
push e cliente Linux/Windows ficam fora desta fatia, conforme o recorte.

## PRs da entrega

1. [Protocolo e monorepo](https://github.com/soyeht/soyeht/pull/1).
2. [Supervisor](https://github.com/soyeht/soyeht/pull/2).
3. [Armazenamento](https://github.com/soyeht/soyeht/pull/3).
4. [Engine](https://github.com/soyeht/soyeht/pull/4).
5. [CLI e instalação Dev](https://github.com/soyeht/soyeht/pull/5).
6. [Aceites sob launchd](https://github.com/soyeht/soyeht/pull/6).
7. [App Mac](https://github.com/soyeht/soyeht/pull/7).
8. [App iPhone](https://github.com/soyeht/soyeht/pull/8).

O PR que contém este relatório fecha os ajustes de conexão, o roteiro e as
instruções de uso. Os checks desse PR são a referência para o fechamento.
