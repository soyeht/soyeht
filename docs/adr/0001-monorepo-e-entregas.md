# 0001 — Monorepo e entrega incremental da fatia 1

Estado: aceito para implementação, 2026-09-10.

## Contexto

O objetivo exige um PR por passo e validações bloqueantes desde o primeiro
PR. A implementação dos seis aceites de processo depende do ptyd e do engine,
que vêm depois do protocolo. O repositório privado não permite proteção de
branch no plano atual. A autorização de execução permite merge por squash após CI verde.

## Decisão

Protocolo, crates Rust, aplicativos novos Swift, design e documentação vivem
no mesmo repositório. A prosa existente foi movida sem alteração de conteúdo
para `docs/contracts`. A análise interna fica fora do repositório. A fonte
de formatos executáveis fica em `contracts/protocol.json`. Licença do projeto:
Apache-2.0, conforme correção explícita do Caio em 2026-09-10.

Cada passo entrega código utilizável no seu nível e verificações reais:
protocolo tem testes de serialização e enquadramento nas três linguagens;
ptyd tem testes de processo; engine introduz os seis aceites completos.
Nenhum job inexistente é representado por um teste que sempre passa.
Os jobs de aplicativos constroem os alvos desde sua introdução no passo
correspondente. Até esses passos, o PR não afirma entregar a fatia inteira.

Main não recebe push direto. Cada mudança vai em worktree e branch próprias,
com PR e merge por squash após CI verde, conforme autorização do Caio. A
revisão posterior pode gerar novos PRs. Se a CI estiver indisponível, os mesmos
checks rodam localmente, com evidência no PR e issue para corrigir o runner.
Não alegamos proteção mecânica da branch no GitHub.

## Motivo

Os passos precisam ser revisáveis sem simular componentes ainda não
implementados. O relatório final só pode declarar a entrega com todos os
gates finais ativos e passando. Essa ordem não altera os aceites nem concede
dispensa de nenhum deles.
