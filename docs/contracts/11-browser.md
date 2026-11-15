# Contract 11 · Browser (web pane)

Status: CLOSED on 2026-09-11; §11 (human mode) added the same day, gated by its spike. Design: screens 13, 13b and 13c in
`design/soyeht-desktop.pen`. Decisions taken without an explicit answer are
marked "(assumed)"; they follow the recommendation given at the time.

## 1. What it is
A fourth pane type next to terminal, agent and editor: `mode: "web"`. It
renders a real web page with the platform's native web view (WebKit on Mac;
the platform web view in the desktop app). It lives in the workspace grid
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
- **Project file**: `web.open {path}` shows a file of the workspace as a page, served by the engine
  (§5b). The agent writes the page into the project and opens it; there is no server to start.

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
`web_open {url | path, workspaceId?, newPane?, incognito?}` → `{paneId}` (§6 for when it answers; `path` is §5b); `web_navigate {paneId,
url | "back" | "forward" | "reload"}`; `web_snapshot {paneId, screenshot?}` →
page title, URL and the accessibility tree with refs `e1…eN` (interactive
and named elements only); `web_click {paneId, ref}`; `web_type {paneId, ref,
text, submit?}`; `web_press {paneId, key}`; `web_hover`; `web_select {ref,
value}`; `web_scroll {ref? | dx, dy}`; `web_wait {text? | selector? |
networkIdle? , timeoutMs}`; `web_evaluate {paneId, js}` (behind a permission
request; never granted by an agent permission mode); `web_screenshot {paneId, fullPage?}`;
`web_read {paneId, screens?}` (§5a);
`web_console {paneId, since?}` (level, text, source and time; never request
or response bodies); `web_network {paneId, since?}` (method, URL, status,
duration and size; never headers or bodies); `web_resize {paneId, preset |
width, height}`; `web_close {paneId}`.

### 5a. Reading a page (ADR 0109)
`web_read` reads a pane the way a person does. Its result is `{navigationGeneration, title, url,
screens, text, links, endOfPage, truncated}` plus one image. A host must:
- read from the top of the page, one screen at a time, up to `screens` (default 4, at most 12),
  scrolling the way the platform's own scrolling does and not by dispatching input events into
  the page; when the document does not scroll but the page's main scroller is an element inside
  it, scroll that element;
- return one image of the screens joined in page order, bounded by the screenshot limit and, because
  the agent receives it as base64 inside one message of its protocol (1 MiB) with the text beside
  it, and a harness may carry the result twice in it, by a quarter of that message (`webAgentImageBytes`,
  256 KiB; scale the picture down first), in which
  a bar that stays fixed on screen appears once and no row of the page is missing at a joint;
- return the visible text of the screens in reading order, each piece once, and the visible
  `http(s)` links with their text, each once;
- run no script of the app in the page's own JavaScript world, and leave no global, handler,
  node, attribute or patched function in it;
- leave the page where the person had it: it puts the scroll position back before it answers,
  also when the read fails, and does not when a person took over;
- stop with `HUMAN_TOOK_OVER` as soon as a person's input reaches the pane, and with an error if
  the pane is resized during the read.

`screens` says how many screens the image holds. `endOfPage` is false when the read stopped before
the page ended (the screen limit, the time budget, or a scroll that did not move); `truncated` is
true when a limit cut the text (64 KiB), the links (100), or what the host reads of a very large
document, or when the answer had to be trimmed to fit its byte budget. `web_screenshot` with
`fullPage` uses the same reading and adds `endOfPage` to its result: it covers at most 12 screens.

How the Mac does it (native page-down, snapshots of the view, an isolated content world) is in
ADR 0109; Linux and Windows implement the same contract with their own web view.

The app's own scripts run in a separate JavaScript world. The page's own world holds only
`web_evaluate`, the console and network capture, and the component probe of Pick element, and the
capture and the probe are used only on a development host (`localhost`, `*.localhost`, `*.local`,
loopback and private network addresses). When the pane leaves a development host the capture is
removed and its readings and counts are cleared: on any other site `web_console` and `web_network`
return nothing. `web_wait` with `networkIdle` is weaker than the page's own request counting
would be: it holds when the document is complete and no resource has finished loading for
500 ms; a request still in flight is not seen.

The pane's user agent carries the tokens of the Safari installed on the Mac
(`Version/<its version> Safari/605.1.15`, read at run time; the major version that ships with the
system when it cannot be read), because sites treat a WebKit view without them as an embedded
browser. It is the same engine and claims no other browser (ADR 0109).

Refs expire at every navigation; a stale ref returns `STALE_REFERENCE` and
the agent must snapshot again. Every agent action is performed in the
visible pane, so the user watches it happen; the pane shows an "agent
driving" badge while a tool runs. Human input in the pane (click, typing,
address bar) pauses agent driving and the next tool call returns
`HUMAN_TOOK_OVER` until the user hands control back from the badge
(contract 05, "your turn").

### 5b. A project file as a page (ADR 0166, section 9)
`web.open` takes `path` in place of `url`: a path relative to the workspace folder. Exactly one of
`url` and `path` is given; both, or neither, is `INVALID_PARAMS`. The engine serves the file read-only on
the loopback interface for the life of the pane, and the pane shows it as it shows any `http` address. The
pane's `url` is that loopback address, so the address bar, `web_snapshot` and every other tool work on it as
on a page of a development server, and its record carries `projectPath`, the path that was opened, so that
the page can be served again (below) and a client that cannot show it can name it.

**What the page can read.** The page's script can read every file below the directory of the file that
was opened, except those with a part of the path that starts with `.` (a dotfile, `.git`): the whole
directory tree, not only the files the page names. A file opened at the workspace root therefore lets the
page read the whole project minus its dotfiles, and `path` at the root is allowed. Nothing above that
directory is served: `path` must stay inside the workspace folder, by the rule the editor uses to open a file
(one function, `resolve_in`, covered by the editor's containment fuzz test), so an absolute path, a `..`
part and a link that leads out of the folder are refused (`INVALID_PARAMS`); it must also name an existing
regular file (a missing file is `NOT_FOUND`), and no part of it may start with `.`, contain `:` or a
backslash, end with a dot or a space, or be the name of a Windows device (`CON`, `NUL`, `COM1`, ...), on
every system, so that a name means the same wherever the project is opened. Every request is checked again
on the canonical path, so a link made after the page opened does not lead out either. A file over
`webProjectFileBytes` is refused, only `GET` and `HEAD` are answered, a request names a file and never a
directory (no listing), and `Range` is answered with `206` (WebKit needs it for audio and video).

**Who can ask.** The address is `http://127.0.0.1:<port>/<token>/<path>`: a port the engine chooses at open
and a random token of 256 bits (the engine's own `auth::token`, compared in constant time). The engine
listens on the loopback interface only. The token keeps out a program or a web page that does not know the
address: another page in a pane, a script that probes local ports, another user of this machine. It does not
keep out whoever can read the address, and every client of the engine can: it is the pane's `url` on the
phone and in the other clients' lists, and in the result of `web_read` and `web_snapshot`. A request whose
`Host` is not the loopback address on the page's port is refused, so that a name that an attacker made point at
`127.0.0.1` (a rebinding) gets nothing even from someone who has learned the token. The sandboxed page has the
origin `null`, so the answers allow it to read them (`Access-Control-Allow-Origin: null`) and no other
origin. At most `webProjectPageConnections` connections are served at once.

**What the page is.** Every answer carries a content security policy that gives the page an opaque origin
(`sandbox allow-scripts`: no cookies, no storage, no windows, no forms, whatever the cookies of the other
development servers on `127.0.0.1` are) and names the page's own address, `http://127.0.0.1:<port>`, for
everything it may load or fetch (`default-src`, `script-src`, `style-src`, `connect-src`, with `data:` and
`blob:` for what the page makes itself; `'self'` is not used because a sandboxed page's own origin is opaque and
WebKit does not match it). The page cannot fetch from the agent, the engine, another local server or the
network, and cannot send what it reads out of the page by a request. Answers are never cached (`no-store`) and
carry `nosniff`.

**The page cannot navigate away.** A pane that holds `projectPath` goes only to addresses with the
scheme, host and port of the address the engine holds for it (`contracts/vectors/project-page-origin.json`,
passed by `soyeht_protocol::web::same_origin` and the Swift client). Every host that renders a web
page refuses any other navigation of that pane: a link, `location`, a redirect, a new window and a command;
a frame of the page may also hold the addresses the page makes itself (`about:blank`, `about:srcdoc`,
`data:` and `blob:`), which the sandbox and the policy above already govern. The engine refuses
`web_navigate` to another origin with `NAVIGATION_REFUSED`. Without this a script could carry a file it
fetched out of the page in an address. The Mac and the desktop client implement it; a client that did not
could be navigated away by the page. Known limit: WebKitGTK reports a top-level and a frame navigation the
same way, so a top-level `data:`, `blob:` or `about:srcdoc` navigation is decided as a frame's and is put
back after it commits.

**The page follows the files, and not the build.** The engine reloads the pane's page when a file that
the page's server has served since the page last loaded changes, once a burst of changes has settled; a
change to any other file under the folder (a build output, `node_modules`, a file the page never asked
for) reloads nothing. A page that is changed again and again is reloaded at a growing interval, from
half a second to half a minute, and the interval is back at its start after a quiet while. The reload is
the engine's own: the command carries `background` in its access (§7), so no host selects the pane, raises its
window or moves focus for it. A pane that has gone to another address is left where it is, and a pane with no
host connected is not reloaded.

**Same file, same pane.** Opening a file that a live pane of the workspace still shows focuses that pane,
unless `newPane` is set; a pane that has since gone to another address is not that pane. A page counts
against `webPanesPerWorkspace` like any web pane.

**When it ends, and after a restart.** Closing the pane ends the server. A pane that holds `projectPath` is
served again when the engine starts: it gets a new port and token, and its `url` is updated, so that the page
is back at every login and update with no new pane. A file that is no longer there leaves the pane as it was.

**Who can load it.** The page is served on the loopback interface of the computer that runs the engine, so
only a client on that computer loads it. A paired phone, which is remote, shows a card with the pane's title
and `projectPath` and the button "open on the Mac", and never the address with its token; it does not render
the page. The `web_open` of an agent for a project page is the navigation of §6 and asks about the session of
`127.0.0.1`, as for any local page; the page itself never has one.

**Not verified.** Windows: the file, the server and the watch are written for it and the path rules have
cases that run on any system, but nobody has run a project page on Windows; the link cases run only on
systems with symbolic links.

## 6. Session, cookies and privacy (assumed)
- One persistent WebKit data store per Soyeht profile on this Mac (Dev and
  Release never share). Logins persist across restarts. An "incognito pane"
  option uses a non-persistent store.
- The agent may not use a signed-in session silently. The first `web_*` call
  that would send cookies for a domain raises `pane.permission_requested
  {kind: "web_session", domain}` with the options "this task only", "always
  in this workspace", "deny". No agent permission mode answers this request.
  The call that opens a page is such a call: an agent's `web_open` of a pane
  that is not incognito makes the pane empty and sends it to the address as a
  `navigate` command, so the host says whether the address's site has cookies
  before the page loads, and the permission request is the same one
  `web_navigate` raises. The answer to `web_open` is `{paneId}` once the host
  accepts the navigation, or the error that stopped it, which can take as long
  as the person needs to answer (the MCP server waits about 70 s). When the
  open fails (a denied session, a host error, a timeout) the empty pane is
  closed and nothing is recorded under the call's idempotency key, so the same
  call made again asks again; made again while the first is still waiting, it
  joins it. A second `web_open` of the same address before the host has
  reported the first pane's address opens a second pane. A pane a person
  opens loads the address at once, and so does an incognito pane, which has no
  session to protect.
- No cookie import from other browsers in v1. Only `http` and `https`
  navigations; everything else is refused with an explicit message.
- Console and network readings are kept in memory only (rings of 500 and
  300 entries) and are never written to the database.

## 7. Engine and clients (contract 08)
The page runs in the client that hosts the pane; the engine keeps the pane
record and routes agent calls to that client.
- `pane.create {mode: "web", url, incognito?}`; pane record fields
  `{url, title, viewport, incognito, projectPath?}` (persisted, contract 07; navigation
  history is not persisted in v1).
- Engine → client request `web.command {paneId, command, args, timeoutMs}` →
  result. `args._soyehtAccess` says who asks and what the session may do: `{session, evaluation,
  background?}`. `background: true` marks a command the engine sends on its own, which is no act of a
  person (the reload of a project page, §5b): a host does it without selecting the pane, raising its window
  or moving focus, whatever the session says. A host that does not know the key reads the command as a
  person's, which is why it is a key and not a new session. A host that holds the page of a pane whose
  `projectPath` is set and whose `url` has a different origin than the page it shows (the engine served it
  again after a restart, §5b) loads the pane's `url`. When no connected client hosts the pane (the Mac app is closed)
  tools fail with `WEB_HOST_UNAVAILABLE`.
- `servers.local.list` → `[{port, pid, command, cwd, title?}]`; event
  `servers.local.changed`. The list leaves out the engine itself and any
  other program whose command starts with `soyeht`: they listen on ports but
  are not a project to open.
- Events `web.navigated {paneId, url, title}`, `web.console_counts {paneId,
  warnings, errors}`, `web.agent_driving {paneId, on}`.
- Errors: `STALE_REFERENCE`, `HUMAN_TOOK_OVER`, `WEB_HOST_UNAVAILABLE`,
  `NAVIGATION_REFUSED`.
- iPhone in v1 shows a web pane as a card with title and URL and the
  button "open on the Mac"; it does not render the page.

## 8. Limits (also added to the table in contract 08)
10 web panes per workspace; snapshot ≤ 200 KiB (truncated with a marker);
screenshot picture ≤ 256 KiB (`webAgentImageBytes`; `webScreenshotBytes`, 2 MiB, stays the largest the engine takes); `web_read` ≤ 12 screens, text ≤ 64 KiB, links ≤ 100; `web_evaluate` result ≤ 64 KiB; console ring 500,
network ring 300; `web_wait` timeout ≤ 60 s; local-server list ≤ 50 entries; a served project file ≤ 16 MiB
(`webProjectFileBytes`: the page's own pictures, fonts and short clips, and each request reads its file whole into
memory, so the limit is also the most one request holds) and 32 connections at once
(`webProjectPageConnections`: a page fetches a few dozen files in parallel when it loads, and a page that
opens connections without end must not take the engine's threads).

## 9. Out of v1
Driving the user's everyday browser (Chrome or Safari) in human mode,
public preview URLs or tunnels, cookie import, extensions, recording,
PDF export, per-request interception, rendering web panes on the iPhone,
a certificate-trust UI for local HTTPS (the system trust store is used).

## 10. Changes to other contracts
- 08: methods, events, errors and limits from §7 and §8.
- 03: the MCP tool list gains the `web_*` tools; the current app's
  `open_web` becomes `web_open`.
- 05: agent web actions feed the state phrase ("testing localhost:5173").
- 07: web pane record persisted; console and network never persisted.
- 04 and 02: new permission kind `web_session`, never answered by an agent permission mode.

## 11. Human mode (computer use)
The tool mode of §5 is for the user's own pages: fast, structured, and
detectable by third-party sites, which does not matter there. Human mode is
for sites that must see a person: job boards, feeds, forms on someone
else's site. It is chosen per task (`web_open {mode: "human"}`) and shown
in the pane as a "human mode" badge. Decision (2026-09-11): human mode
runs **only in the Soyeht web pane**, with the pane's own persistent
profile (§6); the user signs in there once. Driving the user's everyday
browser is out of v1 (§9) and may become its own slice later if needed.

What the agent gets and does:
- Only screenshots of the pane (`web_screenshot`) and the actions
  `web_move {x, y}`, `web_click {x, y}`, `web_type {text}`, `web_press`,
  `web_scroll {dx, dy}`, `web_wait`. No snapshot, no `web_evaluate`, no
  script injected into the page, no console or network reading.
- The pane delivers every action to the web view through the same system
  event path a real click or keystroke takes, so the page receives trusted
  events with user activation. The visible cursor does not move and the
  user keeps the mouse and keyboard; the pane may sit on another Space,
  but it should stay visible (an occluded page is told it is in the
  background, like a background tab).
- The pane, not the agent, decides the rhythm: the pointer travels to the
  target along a curved path with varying speed, clicks have press and
  release delays, typing has per-character variation and occasional
  pauses, scrolling comes in steps. The agent asks for "click here"; it
  never controls timing.
- The web view identifies itself as the Safari of the same macOS version
  (a plain user-agent configuration; the engine is the same). It uses the
  persistent profile of §6, so logins, history and cookies look like a
  person's.

What we do not do: solve CAPTCHAs, forge fingerprints, hide the pane from
the user, or bypass two-factor prompts. When the page shows a CAPTCHA, a
login, a two-factor step or anything the agent cannot pass, the pane
pauses with "your turn", the user completes it, and the agent continues
from the next screenshot. A click by the user inside the pane also pauses
the agent. Some sites forbid automation in their terms; the choice to use
human mode on a given site is the user's, and the pane shows the domain
in the badge so the choice is visible.

**Confirmed by the spike (first item of slice 5).** Before any other work
in the slice: a web pane with fabricated system events and a local test
page that checks, in one run: the event arrives as trusted (`isTrusted`),
a pointer move precedes the click, user activation is granted (a popup
opens and a muted video plays on click), the user-agent equals the Safari
string of the running macOS, no automation flag is present, and typing
timing varies between characters. Then one real site, signed in, read-only
(scroll a feed for two minutes) without a challenge. Both green: human
mode stays as written. Any red: the failing check is recorded in
`docs/reports/` and the fallback for that point is decided before the
slice continues (system events posted to the process, or a macOS virtual
machine with its own browser).

## 12. Place in the slice plan
New slice 5, "Browser", right after the editor. Order inside the slice:
**human-mode spike (§11)** → pane, controls and local servers → link
routing → pick element to composer → MCP tools with snapshot and actions
→ console and network → session permission and incognito → human mode
(cadence, badge, hand-back).
