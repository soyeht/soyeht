# Contract 11 · Browser (web pane)

Status: CLOSED on 2026-09-11. Design: screens 13, 13b and 13c in
`design/soyeht-desktop.pen`. Decisions taken without an explicit answer are
marked "(assumed)"; they follow the recommendation given at the time.

## 1. What it is
A fourth pane type next to terminal, agent and editor: `mode: "web"`. It
renders a real web page with the platform's native web view (WebKit on Mac;
the platform web view in the TypeScript app). It lives in the workspace grid
like every other pane (assumed), so it can be split, resized, moved and
opened several times per workspace (limit in §8). Opening a URL that already
has a web pane in the workspace focuses that pane instead of creating a new
one, unless the caller asks for a new pane.

## 2. How a page gets opened
- **Address bar**: a URL, or a bare host (`example.com` becomes `https://`).
  `localhost`, `127.0.0.1` and `[::1]` with a port become `http://`. There is
  no search engine fallback.
- **New pane** (screen 13c): the empty state lists the development servers
  running on this Mac, read by the engine from the system's listening TCP
  ports (`lsof` on macOS and Linux; a curated port list on Windows), each
  with port, command, working directory and the page title when the server
  answers. The list refreshes every 5 s while the empty state is visible.
- **Links in terminal and agent panes**: a clicked `http(s)` link opens in a
  web pane when the "open links in Soyeht" preference is on. Default: on for
  `localhost` and `127.0.0.1`, off for everything else (those go to the
  system browser).
- **Agent**: through the MCP tools in §5.

## 3. Controls (screen 13)
Back, forward, reload (`Shift` for a reload that ignores the cache), address
bar with a lock or "not secure" mark, open in the system browser, DevTools
(the WebKit inspector), device viewport presets (iPhone SE, iPhone 15 Pro,
iPad, "responsive" with width × height and rotate), zoom, find in page,
screenshot to clipboard or file, and **Pick element** (§4). The status bar
shows the local server (port and process id) when the page is local, the
viewport size, console counts (warnings and errors) and the session state.
Multiple web panes are ordinary panes; there is no tab strip inside the pane.

## 4. Pick element → agent (screen 13b)
Turning "Pick element" on injects a script that highlights the element under
the pointer; `Esc` turns it off; `Shift`-click adds to a multi-selection (up
to 20 elements). The page never receives the selection click. On click the
pane builds a package:
- page URL and title; the shortest unique CSS selector; the element path
  (tag and classes from the root); an HTML snippet (outer HTML capped at
  2 KiB); computed styles (box, color, background, font, border, radius,
  display, position); accessibility role, name and state; the rectangle in
  viewport and page coordinates; a cropped PNG of the element plus a 16 px
  margin (max 640 px wide);
- **component name and source file:line, when the page exposes them**:
  React 18 (fiber debug source), React 19 (owner stack), Vue (the inspector
  attribute written by the Vite plugin, or the component file recorded on
  the element), Svelte (the location metadata Svelte attaches in dev mode).
  Production builds and other frameworks carry no source; the field is
  simply absent and the agent locates the code from the selector and HTML.
The package is attached to the composer of the agent pane the user chooses
(default: the last focused agent pane of the workspace). The user adds the
request and sends; it travels as `pane.prompt {attachments}` (ACP content
blocks: one text block with the package as Markdown, one image block).

## 5. Agent tools (MCP on the engine; contract 03 tool list)
`web_open {url, workspaceId?, newPane?}` → `{paneId}`; `web_navigate {paneId,
url | "back" | "forward" | "reload"}`; `web_snapshot {paneId, screenshot?}` →
page title, URL and the accessibility tree with refs `e1…eN` (interactive
and named elements only); `web_click {paneId, ref}`; `web_type {paneId, ref,
text, submit?}`; `web_press {paneId, key}`; `web_hover`; `web_select {ref,
value}`; `web_scroll {ref? | dx, dy}`; `web_wait {text? | selector? |
networkIdle? , timeoutMs}`; `web_evaluate {paneId, js}` (behind a permission
request; never granted by yolo); `web_screenshot {paneId, fullPage?}`;
`web_console {paneId, since?}` (level, text, source and time; never request
or response bodies); `web_network {paneId, since?}` (method, URL, status,
duration and size; never headers or bodies); `web_resize {paneId, preset |
width, height}`; `web_close {paneId}`.

Refs expire at every navigation; a stale ref returns `STALE_REFERENCE` and
the agent must snapshot again. Every agent action is performed in the
visible pane, so the user watches it happen; the pane shows an "agent
driving" badge while a tool runs. Human input in the pane (click, typing,
address bar) pauses agent driving and the next tool call returns
`HUMAN_TOOK_OVER` until the user hands control back from the badge
(contract 05, "your turn").

## 6. Session, cookies and privacy (assumed)
- One persistent WebKit data store per Soyeht profile on this Mac (Dev and
  Release never share). Logins persist across restarts. An "incognito pane"
  option uses a non-persistent store.
- The agent may not use a signed-in session silently. The first `web_*` call
  that would send cookies for a domain raises `pane.permission_requested
  {kind: "web_session", domain}` with the options "this task only", "always
  in this workspace", "deny". Yolo mode does **not** answer this request.
- No cookie import from other browsers in v1. Only `http` and `https`
  navigations; everything else is refused with an explicit message.
- Console and network readings are kept in memory only (rings of 500 and
  300 entries) and are never written to the database.

## 7. Engine and clients (contract 08)
The page runs in the client that hosts the pane; the engine keeps the pane
record and routes agent calls to that client.
- `pane.create {mode: "web", url, incognito?}`; pane record fields
  `{url, title, viewport, incognito}` (persisted, contract 07; navigation
  history is not persisted in v1).
- Engine → client request `web.command {paneId, command, args, timeoutMs}` →
  result. When no connected client hosts the pane (the Mac app is closed)
  tools fail with `WEB_HOST_UNAVAILABLE`.
- `servers.local.list` → `[{port, pid, command, cwd, title?}]`; event
  `servers.local.changed`.
- Events `web.navigated {paneId, url, title}`, `web.console_counts {paneId,
  warnings, errors}`, `web.agent_driving {paneId, on}`.
- Errors: `STALE_REFERENCE`, `HUMAN_TOOK_OVER`, `WEB_HOST_UNAVAILABLE`,
  `NAVIGATION_REFUSED`.
- iPhone in v1 shows a web pane as a card with title and URL and the
  button "open on the Mac"; it does not render the page.

## 8. Limits (also added to the table in contract 08)
10 web panes per workspace; snapshot ≤ 200 KiB (truncated with a marker);
screenshot ≤ 2 MiB; `web_evaluate` result ≤ 64 KiB; console ring 500,
network ring 300; `web_wait` timeout ≤ 60 s; local-server list ≤ 50 entries.

## 9. Out of v1
Public preview URLs or tunnels, cookie import, extensions, recording,
PDF export, per-request interception, rendering web panes on the iPhone,
a certificate-trust UI for local HTTPS (the system trust store is used).

## 10. Changes to other contracts
- 08: methods, events, errors and limits from §7 and §8.
- 03: the MCP tool list gains the `web_*` tools; the current app's
  `open_web` becomes `web_open`.
- 05: agent web actions feed the state phrase ("testing localhost:5173").
- 07: web pane record persisted; console and network never persisted.
- 04 and 02: new permission kind `web_session`, excluded from yolo.

## 11. Place in the slice plan
New slice 5, "Browser", right after the editor. Order inside the slice:
pane, controls and local servers → link routing → pick element to composer
→ MCP tools with snapshot and actions → console and network → session
permission and incognito.
