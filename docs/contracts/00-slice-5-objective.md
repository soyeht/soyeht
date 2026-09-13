# Objective · New Soyeht slice 5: the browser (web pane)

Self-contained, for execution to completion without questions to Caio. Same
absolute safeguards as slices 1–4 (`00-slice-1-objective.md` §0/§5 and the
slice-4 header): no production, Dev profile `Soyeht.next/dev`, no secrets in
the repo, no tags/releases, no competitor brand names in product docs
(integrations like WebKit/Tailscale and the local dev-server tools a page runs
are fine to name), English-only repo, Apache-2.0, no AI attribution in
commits/PRs. Autonomy rule: ambiguity becomes an ADR and work continues;
self-merge with the green local gate (four ratchets + font/color gates +
restricted-vocabulary scan); victoria reviews each PR after merge for
correctness, contract preservation, design fidelity and safety. Date:
2026-09-13.

## 0. Governing contract and design

`docs/contracts/11-browser.md` (CLOSED 2026-09-11) is the single source of
truth for scope; this objective does not re-decide it. Design: `.pen` screens
**Tela 13** (`cqz1Z`, browser + controls), **Tela 13b** (pick element), **Tela
13c** (new-pane empty state with local servers). Consume the `.pen`; never
change it. The browser adds a **fourth pane mode `web`** next to terminal,
agent and editor.

## Architecture and maintainability (decided first, before any feature)

The **first non-spike PR is a boundary ADR** (like ADR 0042 for the editor).
It must state, and every later PR must hold:

1. **The web view is the client's, the engine never renders a page.** The
   engine owns the pane record and routes agent tool calls to the client that
   hosts the pane (contract §7). A closed host fails tools with
   `WEB_HOST_UNAVAILABLE`. No headless browser in the engine.
2. **Bounded pure cores behind ports.** Anything that can be pure Rust is a
   crate that depends only on `soyeht-protocol` + small libs, never on
   engine/store/ptyd: the local-server reader (parse `lsof`/port output → the
   `servers.local.list` shape) behind a `PortScanPort`; the URL/scheme
   normaliser (`example.com`→https, `localhost:PORT`→http, refuse non-http(s));
   the element-package builder shape and the snapshot/console/network ring
   buffers (500/300) as bounded types. One error enum per crate, mapped once to
   the wire.
3. **The Mac WebKit layer is a bounded module**, colours/fonts only from
   `SoyehtThemeColors`/`MacTheme` (the color/font ratchets already cover
   `apps/mac` and `apps/ios`). The pick-element and human-mode cadence live in
   their own files with unit-testable logic split from the `WKWebView` glue.
4. **Additive wire only.** `PaneMode` gains `web`; new methods/events/errors
   from contract §7/§8 are added; nothing in slices 1–4 changes. `generate.py
   --check` stays green; protocol version stays 1.
5. **Everything bounded** to the contract §8 limits (10 panes/workspace,
   snapshot ≤200 KiB, screenshot ≤2 MiB, `web_evaluate` ≤64 KiB, rings 500/300,
   `web_wait` ≤60 s, local-server list ≤50), enforced in the pure cores and
   re-checked before any wire response.

## 1. Order of work (contract §12 — the spike gates everything)

1. **Human-mode spike (contract §11), FIRST, before any other slice work.** A
   web pane with fabricated system events + a local test page that verifies, in
   one run: the event is `isTrusted`; a pointer move precedes the click; user
   activation is granted (a popup opens and a muted video plays on click); the
   user-agent equals the running macOS Safari string; no automation flag is
   present; typing timing varies between characters. Then one real site, signed
   in, read-only (scroll a feed two minutes) with no challenge. **Both green →
   human mode stays as written. Any red → record the failing check in
   `docs/reports/` and decide the fallback (system events posted to the
   process, or a macOS VM with its own browser) before the slice continues.**
   The spike result is a required report and an acceptance below.
2. Pane, controls and local servers (Tela 13, 13c).
3. Link routing from terminal/agent panes (contract §2).
4. Pick element → agent composer (Tela 13b, contract §4).
5. Agent MCP tools with snapshot and actions (contract §5).
6. Console and network readings (in-memory rings; contract §6).
7. Session permission (`web_session`, excluded from yolo) and incognito
   (contract §6, §10).
8. Human mode: cadence, badge, hand-back (contract §11).

## 2. Acceptance (work ends only when all pass)

1. **[local, procedure/report] Human-mode spike.** The spike above runs on
   Caio's Mac (or via aria on the Dev profile); `docs/reports/human-mode-spike.md`
   records each of the seven checks with its result and, for any red, the
   chosen fallback. Human mode is not built until this is green or its fallback
   is decided.
2. **[CI] Pane and controls.** A `mode:"web"` pane opens a URL by the §2 rules
   (bare host→https, localhost:PORT→http, non-http(s) refused with
   `NAVIGATION_REFUSED`); back/forward/reload, address bar with lock/not-secure,
   open-in-system-browser, DevTools, viewport presets, zoom, find, screenshot
   all work; the status bar shows local server (port+pid), viewport, console
   counts and session state. Re-opening a URL already paned focuses it unless
   `newPane`.
3. **[CI] Local servers.** `servers.local.list` returns the listening dev
   servers (port, pid, command, cwd, title?) from the engine's port read,
   bounded to 50, refreshing on `servers.local.changed`; the new-pane empty
   state (Tela 13c) lists them and refreshes every 5 s.
4. **[CI] Link routing.** An `http(s)` link clicked in a terminal/agent pane
   opens a web pane when the preference is on (default: on for
   localhost/127.0.0.1, off otherwise → system browser).
5. **[CI] Pick element.** Turning it on highlights the element under the
   pointer, `Esc` off, `Shift`-click multi-selects (≤20), the page never
   receives the selection click; the package carries URL/title, shortest unique
   CSS selector, element path, ≤2 KiB outer HTML, computed styles, a11y
   role/name/state, viewport+page rects, a cropped PNG (+16 px, ≤640 px wide),
   and component name + source file:line when the framework exposes it (React
   18/19, Vue, Svelte; absent otherwise). It attaches to the chosen agent
   pane's composer and sends as `pane.prompt {attachments}` (one text/Markdown
   block + one image block).
6. **[CI] Agent tools.** The `web_*` tools of contract §5 work in the visible
   pane with the "agent driving" badge; refs expire on navigation
   (`STALE_REFERENCE`); human input pauses driving (`HUMAN_TOOK_OVER`) until
   hand-back; `web_evaluate` is behind a permission never granted by yolo;
   `web_console`/`web_network` never expose request/response bodies or headers.
7. **[CI] Session and privacy.** One persistent WebKit data store per profile;
   the first cookie-sending `web_*` call per domain raises
   `pane.permission_requested {kind:"web_session", domain}` (not answered by
   yolo); incognito uses a non-persistent store; only http/https; console and
   network live only in memory (never in the database).
8. **[CI] Human mode** (only if acceptance 1 is green as written): trusted
   events with user activation through the system event path, the pane's own
   persistent profile, Safari user-agent, pane-decided cadence (curved pointer
   path, press/release delays, per-character typing variation, stepped scroll);
   the visible cursor does not move; a user click or an unpassable challenge
   (CAPTCHA/login/2FA) pauses with "your turn"; the badge shows the domain. We
   do **not** solve CAPTCHAs, forge fingerprints, hide the pane, or bypass 2FA.
9. **[reviewer] Design fidelity.** victoria compares the running web pane
   against `.pen` Tela 13, 13b and 13c on palette, spacing, type, radii, shadow
   and each control's shape/placement, and pastes the image pairs in
   `docs/reports/slice-5.md`.
10. **[CI] iPhone.** A web pane shows on the phone as a card (title + URL +
    "open on the Mac"); it does not render the page (contract §7).
11. **[CI] No regression.** Every slice 1–4 acceptance and test still passes;
    the four ratchets, the font and color gates and the restricted-vocabulary
    scan are green; `generate.py --check` clean; protocol version still 1.

## 3. Outside this slice (contract §9)

Driving the user's everyday Chrome/Safari; public preview URLs or tunnels;
cookie import; extensions; recording; PDF export; per-request interception;
rendering web panes on the iPhone; a certificate-trust UI for local HTTPS.

## 4. Workflow and delivery

Small PRs (one core, one contract slice, or one screen each), green local gate,
self-merge, an ADR per ambiguity. Design-fidelity acceptance on every screen.
On completion, `docs/reports/slice-5.md` with the per-screen image pairs and the
spike report link, and a message to victoria with the PR links. victoria
reviews each PR for correctness, contract preservation, design fidelity and —
for the agent tools and human mode — safety (the "what we do not do" bounds,
body/header redaction, the yolo exclusions, path/permission scoping).
