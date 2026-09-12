# Objective · design alignment pass 1: the Mac app to the `.pen`

Self-contained, for execution to completion without questions to Caio. Same
absolute safeguards as the slices (`00-slice-1-objective.md` §0 and §5: no
production, Dev profile `Soyeht.next/dev`, no secrets, no tags, no competitor
names, no assistant attribution, Apache-2.0). Same autonomy rule: ambiguity
becomes an ADR and work continues; self-merge with the green local gate;
victoria reviews after each merge. Keeps the hardening round 1 ratchets green.
Date: 2026-09-12.

## Why this pass exists

The built Mac app diverged from the approved designs in
`design/soyeht-desktop.pen`. Root cause, owned by the reviewer: the slice
objectives said "neomorphic style from the `.pen`" and named screens, but that
is a pointer, not a fidelity requirement; there was no design-fidelity
acceptance, and the reviews checked only behaviour. A functional acceptance
stays green while the pixels are wrong. This pass restyles the existing Mac
screens to match the `.pen`, and from now every UI objective carries a
design-fidelity acceptance (see §5).

This pass changes only presentation. It adds no protocol method, event, field
or error; no engine behaviour; no store schema. Every slice 1, 2 and 3
functional acceptance and every automated test keeps passing unchanged. The
four hardening ratchets and the restricted-vocabulary scan run before each
merge.

## 0. Source of truth and how to read it

`design/soyeht-desktop.pen`, opened through the pen MCP (`get_app_state`,
`execute`, and the Code panel's HTML/Tailwind export or the `code` skill).
Frames and their node ids (confirm the rest with `get_app_state`):

- Tela 1 · Workspace com panes — `l5YU91`
- Tela 2 · Pane nova (entrada) — `nOYUL`
- Tela 3 · Tarefas do workspace (board) — `N0IEF`
- Tela 4 · Tarefa aberta (detail) — `JQXi0`
- Tela 5 · Agente via ACP
- Tela 6 · Loop da pane
- Tela 7 · Tarefas em lista (list)
- Tela 9 · Preferências
- Tela 10 · Nova tarefa (dialog)
- Tela 12 · Barra lateral de apps e consumo — `p4Q8vX`
- Tela 13 · Navegador (pane web) — `cqz1Z`

Reusable components: `c0OyQ` task card, `QymLR` task pane (board), `WauZf`
task detail, `tdMRT` task list, `WfEHw` new-task dialog, `ytwCn` agent pane,
`QzNZM` pane card, `O7jqQ` harness selector, `gnl6l` loop panel, `ZZheU`
preferences, plus the buttons, tab and field components in the `Componentes`
frame.

Design laws: the `neomorphism-macos` skill and the read-only sources it names
(soft blue-grey ground, dual light/dark shadow for raised and inset surfaces,
the accent and semantic colours, the type scale, spacing and radii). Read them
before writing any style code.

## 1. What this pass covers

Only the screens the Mac app already ships (slices 1–3). Later slices restyle
their own new screens as they land, each with the §5 acceptance.

1. **One shared neomorphic theme, from the `.pen` tokens.** Replace the ad-hoc
   `MacStyle.Palette` with the exact tokens the `.pen` uses: ground/canvas,
   raised surface, inset surface, ink and secondary text, accent, the three
   semantic colours (urgent, warning, success), the shadow colour and its two
   offsets, the corner radii, the spacing scale and the type ramp (family,
   sizes, weights, line height). Read them from the `.pen` (export a frame to
   HTML/Tailwind, or the `code` skill) rather than guessing. Keep the theme in
   one file so every screen shares it, and record the token→source mapping in
   the closing report.
2. **Workspace and panes (Tela 1, Tela 2, Tela 5, component `ytwCn`).** The
   window shell, the pane grid, the new-pane entry, the harness · model ·
   effort selector and the agent pane chat match their frames.
3. **Task board (Tela 3, components `QymLR`, `c0OyQ`).** Columns, card layout
   and spacing, the state phrase and priority, the YOUR TURN / SYSTEM'S TURN
   badge, the "Quadro / Lista" toggle, the `Despachar` and `Nova tarefa`
   pills, and the attention "N need you" affordance match the frame.
4. **Task list (Tela 7, component `tdMRT`).** The table columns (chave,
   tarefa, estado, prioridade, dono, atualizado) and the state chips match.
5. **Task detail (Tela 4, component `WauZf`).** Description, how-to-verify,
   must-not-change, subitems, history and comments, and the drift review with
   the `contractChanges`/`outOfScopeEdits` lists match the frame.
6. **New-task dialog (Tela 10, component `WfEHw`).** Fields, the required
   must-not-change field, and the two create buttons match.
7. **Loop and preferences (Tela 6 `gnl6l`, Tela 9 `ZZheU`).** The loop and
   lead controls and the preferences window match.

## 2. Method

For each screen: open its `.pen` frame through the pen MCP, take its exact
tokens and layout (export to HTML/Tailwind or read via the `code` skill), then
reproduce it in SwiftUI against the shared theme of item 1. Do not invent
spacing, colour or type; take them from the frame. Where a `.pen` construct
has no clean SwiftUI equivalent, record the deviation in an ADR and pick the
closest faithful rendering. Behaviour, bindings and the task replica stay
exactly as they are; only the views and the theme change.

## 3. Acceptance (work ends only when all pass)

1. [reviewer] For each screen in §1 items 2–7, victoria compares a screenshot
   of the running Mac app against a render of the matching `.pen` frame and
   confirms they match on: palette (every colour from the shared theme, none
   ad-hoc), the spacing scale, the type ramp, corner radii, the raised/inset
   shadow treatment, and each component's shape and placement. A divergence on
   any of these axes fails the screen. The closing report
   `docs/reports/design-alignment-1.md` pastes, per screen, the two images and
   the pass mark.
2. [CI] Every slice 1, 2 and 3 functional acceptance and every automated test
   still passes unchanged; no test, acceptance or gate step is removed
   (`check-verification-not-weakened` green). The task replica still does not
   compute state.
3. [CI] The four hardening ratchets and the restricted-vocabulary scan are
   green. No change to `contracts/protocol.json`, the engine, the store schema
   or `design/soyeht-desktop.pen`.
4. The shared theme is one file; no screen defines its own colours, radii or
   shadows outside it; the token→`.pen` mapping is in the report.

## 4. Out of scope

New screens or features; any engine, protocol, store or CLI change; the editor
and browser slices; the iPhone app (its own alignment pass later); editing the
`.pen` designs themselves (this pass consumes them, it does not change them).

## 5. Process fix (applies from here on)

Every objective that builds or changes a client screen carries a
design-fidelity acceptance like §3 item 1: name the `.pen` frame, require
faithful reproduction against the shared theme, and have victoria compare the
running screen to the `.pen` render before the screen is accepted. The slice 4
and 5 objectives (editor, browser) will each carry it.

## 6. Workflow and delivery

Small PRs, one screen or the shared theme per PR, green local gate with the
real restricted vocabulary and the four ratchets, self-merge, an ADR per
ambiguity. On completion, `docs/reports/design-alignment-1.md` with the
per-screen image pairs and the token mapping, and a message to victoria with
the PR links. victoria reviews each PR for fidelity, not only for the gate.
