# Fatia 1 · a menor prova de ponta a ponta

Estado: recorte definido e liberado pela revisão (aria, 2026-09-10).
Aguardando o "pode começar" do Caio. Nenhum código ainda.

## O que entra
- Perfil **Dev** (dados em pasta própria, porta própria; o Soyeht atual não
  é tocado).
- Engine (Rust) e `ptyd` (Rust) como dois serviços do launchd (contrato 07).
- Esquema do protocolo (`contracts/protocol.json`) com os métodos e eventos
  desta fatia e tipos gerados para Rust, Swift e TypeScript (contrato 08 §1.7).
- CLI: `soyeht pane create --shell` cria UMA pane de terminal cru.
- App Mac novo (Swift): conecta pelo socket local, mostra a pane, tela e
  entrada de teclado.
- App iPhone novo (Swift): pareia pelo QR na tailnet (08 §2.2), conecta,
  mostra a mesma pane (só leitura).
- Banco SQLite único, versionado, `synchronous=FULL` (07).
- Segmentos de tela com replay por offset, `tail`, delta e `gap` (08 §6).

## Aceite (todos precisam passar, na CI e à mão)
1. Fechar e reabrir o app Mac: a pane continua, tela volta sem buraco.
2. `kill -9` no engine e reinício pelo launchd: o processo do shell é o
   MESMO (pid igual), o engine adota pelo `ptyd`, a tela continua, o
   iPhone reconecta sozinho e mostra o mesmo conteúdo.
3. Desconectar e reconectar o iPhone (modo avião 1 min): `hello` →
   `snapshot` → `subscribe`, sem evento perdido nem repetido.
4. Fechar a pane pelo Mac: some no iPhone; reiniciar o engine não a
   ressuscita.
5. Repetir `pane.create` com a mesma `idempotencyKey`: uma pane só.
6. Encher a retenção do `ptyd` com o engine parado: ao voltar, aparece
   "trecho perdido", nunca continuidade falsa.
7. Reiniciar o Mac (teste separado): o shell antigo morre; layout, cwd e
   tela gravada voltam; a pane nasce como shell novo, marcado.

## O que NÃO entra
Pane de agente (ACP), lista de tarefas, loop, chefe, troca de harness,
app Linux/Windows, push, estado por gancho/regex.

## Fatia 2 (depois, já decidida a direção)
Um harness ACP v1 numa pane de agente: prompt, permissão, cancelamento,
recuperação depois de reiniciar o engine (harness vivo no `ptyd`).
