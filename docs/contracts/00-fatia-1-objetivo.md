# Objetivo · Fatia 1 do Soyeht novo (para execução autônoma até o fim)

Este documento é autossuficiente. Quem executa não deve fazer perguntas ao
Caio: toda ambiguidade vira uma decisão registrada em `docs/adr/NNNN-*.md`
(uma página: contexto, decisão, motivo) e o trabalho segue. Padrão: "isso
aqui é uma empresa profissional; quero código profissional; nunca pensa em
bagulho zoado". Data: 2026-09-10.

## 0. Guardas absolutas
1. NUNCA tocar em `~/Documents/SwiftProjects/iSoyehtTerm`, em
   `~/Documents/theyos`, nem nos dados de produção
   (`~/Library/Application Support/Soyeht`, `Soyeht Dev`, `SoyehtDev*`),
   nem nos LaunchAgents `com.soyeht.engine` / `com.soyeht.ptyd`. Esses
   são o produto que o Caio usa AGORA. Só leitura, como referência.
2. Tudo do novo produto vive no repositório novo e usa nomes próprios:
   dados em `~/Library/Application Support/Soyeht.next/dev/`, LaunchAgents
   `com.soyeht.next.dev.engine` e `com.soyeht.next.dev.ptyd`, porta 8291,
   bundle ids `com.soyeht.next.dev.*`. Isso é o **perfil Dev**: o Caio
   continua usando a produção enquanto os agentes testam aqui.
3. Sem segredos no repositório. Sem tags ou releases. Sem publicar nada
   fora do GitHub do próprio repositório.
4. Os contratos em `~/Documents/soyeht/contratos/` (01–08, 00-fatia-1) são
   a fonte da verdade do COMPORTAMENTO. Onde o código e o contrato
   discordarem, o código está errado. Se um contrato for impossível de
   cumprir, registrar ADR explicando e parar naquele ponto com relatório.
5. Não copiar código do produto atual sem revisar; pode reescrever ideias
   provadas (formato dos segmentos de PTY, pareamento HMAC, APNs) citando a
   origem no ADR. Não copiar código de repositórios AGPL nem de
   proprietários; MIT/Apache com atribuição, se copiar algo.

## 1. Entrega
Repositório GitHub `soyeht/soyeht` (criado; privado; licença Apache-2.0), monorepo:
```
contracts/            protocol.json (fonte única) + vetores de teste + gerador
engine/               Cargo workspace: soyeht-engine, soyeht-ptyd, soyeht-cli, soyeht-protocol (tipos gerados), soyeht-store
apps/mac/             Xcode: app Swift (SwiftUI/AppKit) para macOS, alvo Dev
apps/ios/             Xcode: app Swift para iPhone, alvo Dev (pode ser o mesmo projeto com dois alvos)
apps/desktop/         (vazio nesta fatia; TypeScript, Linux/Windows, depois)
design/               soyeht-desktop.pen (copiar de ~/Documents/soyeht/design/)
docs/contracts/       cópia versionada dos contratos 01–08 e 00-fatia-1 (a análise interna NUNCA entra no repositório)
docs/adr/             decisões
.github/workflows/    CI
```
Toolchains fixados: `rust-toolchain.toml` (stable atual), Xcode/Swift
versão registrada em `docs/toolchains.md`, `Cargo.lock` commitado.

## 2. O que construir (escopo exato, nada além)
1. **`contracts/protocol.json`**: JSON Schema (subconjunto explícito,
   documentado) com TODOS os métodos, eventos, erros e o cabeçalho
   binário do contrato 08 que a fatia usa: `auth.challenge`, `hello`,
   `welcome`, `state.snapshot` (+ `state.snapshot_part`),
   `events.subscribe`, `workspace.*`, `pane.create|close|rename|focus|
   resize|input|stream.subscribe|stream.unsubscribe`, `pane.record.set`,
   `device.pair.start|decide|list|revoke`, `pair.begin|prove|result`,
   `engine.status`, `settings.get|set`; eventos `workspace.*`, `pane.created|
   closed|renamed|state_changed`, `stream.replay_start|replay_done|gap`,
   `device.pair_requested|paired|revoked`, `engine.updating|restarted`,
   `ptyd.reconnected|lost`; erros do 08 §7; limites do 08 §8. Gerador de
   tipos para Rust (serde), Swift (Codable) e TypeScript, com teste de ida
   e volta por mensagem e por vetor binário. CI falha se gerado ≠ commitado.
2. **`soyeht-ptyd`**: supervisor de processos do contrato 07 (só PTY nesta
   fatia; a interface já prevê pipes): registro em arquivo, `instanceKey`
   idempotente, retenção 8 MiB com `gap`, `seq`/`ack`, geração do engine
   com recusa de geração menor, socket próprio com versão. LaunchAgent
   Dev com KeepAlive.
3. **`soyeht-engine`**: launchd KeepAlive; SQLite único (`user_version`,
   migrações numeradas, WAL, `synchronous=FULL`); estado de workspaces e
   panes; segmentos de tela (512 KiB, 32 MiB por pane, replay por offset,
   registro `gap`); adoção do `ptyd` ao subir (reconciliação antes de
   marcar pane morta); servidor do protocolo no socket Unix (`0700`/`0600`)
   e no WebSocket em texto puro nos endereços do Tailscale + loopback
   (nunca `0.0.0.0`; LAN só com janela de pareamento aberta); handshake
   `auth.challenge → hello → welcome`; pareamento QR (08 §2.2) com
   Ed25519/HMAC-SHA-256 e consentimento; snapshot em partes + eventos com
   cursor na mesma transação; `idempotencyKey` obrigatória e persistida;
   fila de saída 8 MiB com `slow_consumer`; estados da pane
   `iniciando|trabalhando|ociosa|morta|recuperando` derivados só de fatos
   do processo (sem gancho, sem regex nesta fatia); logs estruturados sem
   segredos, com id de operação.
4. **`soyeht-cli`**: `soyeht --profile dev status | pane create --shell |
   pane list | pane close <id> | device pair-start | device list`.
5. **App Mac (Swift)**: janela com a lista de panes e um terminal (xterm
   nativo em Swift; escolher biblioteca MIT/Apache e registrar em ADR; o
   design segue `design/soyeht-desktop.pen`, estilo neomórfico, Tela 1);
   conecta pelo socket local; `snapshot + subscribe`; entrada de teclado e
   redimensionamento; botão "Adicionar iPhone" que mostra o QR; diálogo de
   consentimento do pareamento.
6. **App iPhone (Swift)**: lê o QR, pareia, conecta pelo Tailscale, mostra
   a lista de panes e a tela da pane (só leitura); reconexão com
   `hello → snapshot → subscribe` ao voltar do fundo; `Info.plist` com
   `NSAllowsArbitraryLoads` e o comentário de justificativa (contrato 06).
7. **CI (GitHub Actions)**, bloqueante em todo PR: `cargo fmt --check`,
   `cargo clippy --all-targets -D warnings`, `cargo test`, geração do
   protocolo sem diff, build do app Mac e do app iOS (xcodebuild, sem
   assinatura de distribuição), e o teste de ponta a ponta abaixo em
   runner macOS. Branch `main` protegida: PR obrigatório, 1 aprovação, checks
   obrigatórios. Commits em Conventional Commits.

## 3. Aceite (o trabalho só termina quando os 7 passam, automatizados onde marcado)
1. [CI] Fechar e reabrir o cliente (simulado pela CLI/teste): pane continua, replay sem buraco.
2. [CI] `kill -9` no engine + reinício pelo launchd: shell com o MESMO pid; engine adota pelo `ptyd`; tela continua; cliente reconecta e vê o mesmo conteúdo.
3. [CI] Desconectar/reconectar cliente: `hello → snapshot → subscribe`, nenhum evento perdido nem repetido (teste injeta mudanças durante a desconexão).
4. [CI] Fechar a pane: some para todos os clientes; reiniciar o engine não a ressuscita.
5. [CI] Repetir `pane.create` com a mesma `idempotencyKey`: uma pane só.
6. [CI] Encher a retenção do `ptyd` com o engine parado: ao voltar, evento `stream.gap`; nunca continuidade falsa.
7. [manual, documentado com roteiro] Reiniciar o Mac: shell antigo morre; layout, cwd e tela gravada voltam; pane marcada "shell novo". Mais: pareamento real do iPhone pelo QR e leitura da pane com Wi-Fi desligado (Tailscale).

## 4. Como trabalhar
- Ordem: contrato/esquema → ptyd → engine → CLI → testes e2e → app Mac →
  app iPhone → roteiro manual. Cada passo é um PR pequeno com CI verde.
- Toda decisão que o contrato não fecha vira ADR; não perguntar.
- Sem "por enquanto": nada de TODO deixado para depois sem issue aberta.
- Ao terminar: relatório em `docs/reports/fatia-1.md` com: o que passou,
  o que não passou e por quê, decisões (ADRs), como rodar tudo do zero
  num Mac limpo, e a lista de riscos conhecidos. Mandar o resumo por
  mensagem para a victoria (que revisa contra os contratos) com os links
  dos PRs.

## 5. Guarda adicional (Caio, 2026-09-10): nomes de concorrentes NUNCA no repositório
É proibido escrever, em qualquer arquivo versionado (código, docs, ADR,
comentário, mensagem de commit, PR), o nome de qualquer outro produto ou
empresa concorrente. Integrações que o produto usa (Claude Code, Codex,
Gemini, Cursor CLI, Tailscale, GitHub, ACP, worktrunk) podem ser citadas.
Implementar na CI, no primeiro PR, um check bloqueante `forbidden-terms`:
a lista de termos NÃO existe no repositório de forma nenhuma (nem em
hash: palavra curta é adivinhável). Ela vive num segredo do repositório
(`FORBIDDEN_TERMS`, um termo por linha, mantido pela victoria); o script
lê o segredo do ambiente, tokeniza todo arquivo de texto (minúsculas;
tokens e pares consecutivos) e as mensagens de commit do PR, e falha se
bater. A mensagem de falha mostra só arquivo e linha, nunca o termo, para
o log da CI também não vazar. Se o segredo não estiver disponível (PR de
fork), o check falha fechado. Para citar origem de ideia, escreva "análise
interna".
