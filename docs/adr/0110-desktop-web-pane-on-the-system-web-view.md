# ADR 0110: The desktop web pane draws the system's web view off screen

- Status: accepted for Linux (phase 1); Windows is settled by ADR 0124
- Date: 2026-10-25

## Context

The Mac client has a web pane (ADRs 0055 to 0060) and is the engine's web host: an agent's
`web_snapshot` and `web_screenshot` reach the engine, which routes `web.command` to the one
connection that claimed the `web_host` capability. The desktop client (ADR 0084) had no web view,
and a web pane drew a notice.

The goal is the Mac's: a person opens a site in a pane with their own login, and an agent reads
the page (a full-page screenshot and the text) to summarize it, for example a timeline they are
signed in to. Many sites block known automation, so the pane must be a real browser on the
system's own engine, with the person's own session:

- no automation tool or protocol (Playwright, Puppeteer, Selenium, WebDriver) and no remote
  debugging port or inspector server;
- no spoofed user agent or fingerprint, and nothing done about captchas;
- our scripts run in an isolated script world the page cannot see.

The engine's host protocol is unchanged (`web.command`, `web.host.navigated`, the `web_host`
capability, attachments before the answer). This ADR is about the client: how a web view lives in
a GPUI window on Linux, how it is captured, and what Windows needs.

## Decision

### The web view is drawn off screen into the window

Three ways to put a system web view in a GPUI window were weighed. The evidence is from Ubuntu
26.04 (GNOME on Wayland, llvmpipe, WebKitGTK 2.52.6), where each was tried or checked against the
libraries' own documentation.

| | Wayland | X11 | Overlays above the page | Capture | Input and IME |
| --- | --- | --- | --- | --- | --- |
| Native child surface (`GtkPlug`, a reparented window) | No: a client cannot make a child of a surface of another toolkit's connection | Yes | No: a native child is always on top of what GPUI draws, so menus, popovers and rounded corners are cut by it | the engine | the system's |
| A top-level window kept over the pane | No: `xdg_toplevel` has no position | Yes (XWayland) | No, and a second window to focus, stack and close | the engine | the system's |
| **The page drawn off screen and painted by GPUI** | **Yes** | **Yes** | **Yes: it is an image in GPUI's own scene** | the engine | forwarded by us |

The pane uses the third. It is the only one that is the same on Wayland and X11, keeps GPUI's
menus, popovers and rounded card above the page, and needs no second window. The cost is that the
window forwards input and shows pictures, which the other two do not.

`soyeht-webview` loads GTK 3 and WebKitGTK 4.1 when the first pane opens and runs them on one
thread of its own with a GLib main loop (GTK wants its own loop and one thread). A page lives in a
`GtkOffscreenWindow`. When its window is damaged the thread reads the surface (at most 30 times a
second), keeps the latest picture and tells the window, which paints it with
`Window::paint_image` (and drops the previous one from the atlas).

What was checked on the VM:

- The offscreen window renders a page on Wayland and the GDK damage signal fires about 36 times a
  second for a page that animates.
- A `GdkEvent` delivered with `gtk_main_do_event` reaches the page as input of a device: a page
  that listens in the capture phase sees `mousedown`, `mouseup`, `click`, `keydown`, `input`,
  `keyup`, `wheel` and `scroll`, all with `isTrusted` true, and a pointer move precedes a click.
- `webkit_web_view_get_snapshot` takes the engine's own picture of the pane (2200 by 1520 for a
  1100 by 760 pane on this screen). Its whole-document region exists and gave a 2200 by 5652
  picture of a long page, but it holds only what the page has drawn so far, so a full-page
  capture is the walk of `web.read` instead (below).
- A website data manager with a base data directory and a cookie storage file keeps a cookie
  across two runs of the program.

### Why not a wrapper (wry)

`wry` was read, not adopted: its documentation says `build_as_child` is X11 only on Linux and
"won't work on Wayland" (Wayland needs a `gtk::Fixed` it builds in a GTK window, which is the
separate window above), it has no capture of a page or of the whole document, and its
initialization scripts and `evaluate_script` run in the page's own world, which this feature must
not do. It would bring the `tao` and `gtk-rs` trees for a surface we cannot use. The default of a
thin layer holds.

### Why our own FFI loaded at run time, not the gir crates

The `webkit2gtk` and `webkit6` crates link WebKitGTK when the program is built (pkg-config needs
the development packages) and pull in the `gtk-rs` tree. The gate hosts, a contributor's
machine and the Fedora and Omarchy VMs build this client without WebKitGTK's development files,
and the built program must start on a machine that has no WebKitGTK (the pane then says so). So
`soyeht-webview` declares the sixty-odd functions it calls from the public GTK 3, GDK, GLib, cairo
and WebKitGTK headers and loads them with `libloading` (already in GPUI's lock file). All of it is
in `linux/ffi.rs`; every `unsafe` block says why it holds. The signatures are the libraries'
documented ones, and the ABI of a major version does not change; `Api::load` fails with a message
for a library or a symbol it cannot find, and a test of the program on the VM exercises them.

GTK 3 and WebKitGTK 4.1 were chosen over GTK 4 and 6.0 because GTK 4 has no off-screen window:
a 6.0 view only paints when it is mapped. The 4.1 API is still shipped and maintained (Ubuntu
26.04, Fedora 44, Arch). The way on to 6.0 is the same thread with `webkit_web_view_get_snapshot`
polled for the picture (the call does not need the view to be mapped); that is a follow-up when a
distribution drops 4.1.

### What a page can and cannot see

ADR 0109 lists what a page must not see of the Mac app; this pane keeps to the same rules and
adds what the system web view needs.

- No automation interface is started or allowed: the program refuses to open a web view while
  the environment asks for an inspector server, never sets the property that turns on WebKit's
  automation mode, never enables developer extras, and a unit test fails when the crate names an
  automation tool or a debugging switch. `navigator.webdriver` is therefore false.
- **User agent.** The pane identifies itself as the engine's normal browser, never as an embedded
  view (decision of the owner, 2026-10-25). On Linux that is WebKitGTK's own default user agent,
  which already carries the `AppleWebKit` and `Safari` tokens. The pane sets no application name,
  adds no token that marks it as Soyeht or as embedded, and does not claim to be another browser.
  The language, the fonts and the other properties are the engine's own; nothing is spoofed.
  Captchas, sign-in and two-factor steps are for the person.
- **Our scripts run in one isolated world.** A script runs through
  `webkit_web_view_evaluate_javascript` with a world name. The page cannot read or write what a
  script defines there (checked: a global set in the world is `undefined` to the page, and a page
  global is `undefined` to the script), and cannot patch the natives the script calls, because
  each world has its own wrappers. The page world holds nothing of ours: no user script, no
  message handler (`window.webkit` does not exist), no patched function, no global, no node, no
  attribute, no listener. Console and network capture, `web.evaluate` and the component probe,
  which need the page's own world, are not part of this client.
- **A read is the scrolling of a person.** The scripts of a read only read: they add no node,
  set no attribute, add no listener and dispatch no event (a unit test holds the source to it).
  The page is scrolled with the engine's own wheel events, which the page sees as a person's
  wheel (`wheel` and `scroll`, trusted). WebKitGTK has no page-down action of the view, as
  `scrollPageDown` is on the Mac, so the wheel is the closest thing to it, and the walk measures
  how far a line of it moves the page from the first scroll. The agent's `scroll` command, as on
  the Mac, is a `scrollBy` from the world.
- The agent does not click or type in this phase: those commands answer `internal_error` as the
  Mac's host does for a command it lacks, so no synthetic event is made.
- Pointer, wheel and keys of the person are events of a device (above). A person's input in the
  pane stops an agent from driving it until the person hands it back, as on the Mac
  (a command answers `human_took_over`). The "agent is driving" badge (`web.agent_driving`) is not
  sent by this host yet.
- A page cannot open a window (a request for one is loaded in the same view), ask for a
  permission (refused), show a dialog (an alert is closed, a question is answered "no", leaving a
  page is allowed), choose a file or go full screen; none of those has a window of ours to
  appear in. A navigation to anything but `http`, `https`, `about:blank` and `blob:` is
  ignored, and a response the engine cannot show is not downloaded.

### The web host

`soyeht_client::web_host` is the browser host. It answers `web.command` as the Mac does:

- `snapshot`: the page read in the world into nodes of `role`, `name`, `state`, `value` (the
  address of a link) and `rect`, in document order, for controls and headings and for the text
  of the page (a node of role `text` for each element that holds text of its own that no control
  or heading already names). It is cut to the contract's 200 KiB and says so.
- `read` (`web.read`, ADR 0109): the page walked from the top, a screen at a time, up to the
  screens asked for (default 4, at most 12) and until 80% of the command's time has passed. At
  each screen it waits for the scroll and what it loaded to settle, reads the visible text and
  links, takes the engine's own picture of the pane and scrolls on. The pictures are joined by
  the offsets the page reported, each later one adding only the rows below what is covered
  (`stitch.rs`, whose cases are `contracts/vectors/web-read.json` and are the Mac's too); the
  text of a block seen again at the same place, and of a pinned band, is kept once
  (`read_content.rs`, same vector). The walk keeps to these rules:
  - the page is left where the person had it, on every way out of the walk, except when a
    person has taken the pane over, who is not scrolled again;
  - a person's input is looked for before every scroll, and ends the read with `human_took_over`;
  - when the document does not scroll but a large element inside it does (an application shell
    keeps its feed in one), that element is scrolled and the screens are the rows of it;
  - a resize of the pane ends the walk with the screens it has;
  - a band pinned to an edge is cut from the screens that would repeat it, and a scroll moves
    only as far as what the band leaves free, so there is no seam;
  - a pane that is still taking its size (a card shown or hidden just before the command ran) is
    waited for before the walk takes its reference size, and a resize after that ends the read
    with an error;
  - the answer stays inside the bytes the agent can receive: a picture is a PNG of at most
    a quarter of the harness's message (`ACP_MESSAGE_BYTES`, 1 MiB), the contract's
    `webAgentImageBytes` (256 KiB, shared with the Mac host and the MCP server, which leaves a larger one out and says so), because the agent
    gets it as base64 in one message of its protocol with the text beside it, a harness may carry
    the result twice in it (content and raw output), and a harness drops its connection on a
    longer message (seen with 1.4 MB and 640 KiB pictures). The text carries the page; the picture is
    scaled down and then cut to its top rows; the links and then the text are cut to what is
    left of the contract's JSON budget and of that message; each cut sets `truncated`; a long
    page is never refused. `truncated` is also set when the text reaches 64 KiB or the links 100.
- `screenshot`: the visible region, or with `fullPage` the same walk without the text (the
  engine's snapshot of a region taller than the pane is not used: it does not hold what the
  page loads as it scrolls). Either is a PNG of at most the size above (never over the contract's
  2 MiB) and 4 million pixels, sent as
  attachment frames before the answer, as the engine requires.
- `navigate`, `scroll` and `wait` (text, selector and a quiet page; the host cannot count
  requests without a script in the page, which it will not add).
- The session modes of the engine's note in the arguments (`human`, `incognito`, `probe`,
  `granted`): a probe asks the profile whether the site has cookies before anything runs.
- `click`, `type`, `press`, `hover`, `select`, `evaluate`, `console`, `network` and `resize`
  answer `internal_error` in this phase. `evaluate`, `console` and `network` need a script in the
  page's own world, which the pane does not allow itself; the others are a next step.

The first window of a profile to connect claims the role; a second one is refused by the engine
(`web_host_unavailable`), connects again without it and tries again at its next connection.

The page lives in the profile directory (`browser/web`, `browser/web-cache`, owner only) of the
engine's socket, so a throwaway engine has a throwaway profile. An incognito pane uses a private
context that keeps nothing.

### What the engine does with a large tool result, and how long an agent waits

Two engine rules were found by reading real pages through a harness, and both apply to every
host:

- A tool call is one row of the transcript, bounded to 256 KiB (`TRANSCRIPT_MESSAGE_BYTES`). A
  result with page images is larger, and the engine refused the update, so the row stayed in
  progress for ever. A row that does not fit now loses its `rawOutput`, and then its `content`,
  and keeps its title, status and input: the call completes and the agent still has the result
  from the tool itself (`transcript::fit_tool`).
- The MCP server waited 10 s for every answer. A web command runs in the host and may take as
  long as the longest wait the contract allows (`webWaitTimeoutMs`, 60 s) plus a navigation or
  a read of a long page; the server now waits that long and a margin for the host commands and
  `web.navigate`, and 10 s for the others, `web.open` and `web.close` among them, which answer
  at once (`reply_timeout`, which asks `soyeht_protocol::web::waits_for_host`). The engine's own
  default for a web command stays 30 s. A call that times out is sent once more, as any call that
  loses its connection is, and a question the engine puts to the person (a web session, a
  permission) can wait longer than this server does, as before.

A harness that defers the schemas of MCP tools (it must load a tool before it may call it) can
call a Soyeht tool blind with a guessed argument; the harness itself refuses that call
("Invalid tool request") before it reaches the server, and it then loads the schemas and calls
`web_open` and `web_read` correctly. The tool descriptions say that `paneId` comes from `web_open`.

### The window

A web pane is an address line, back, forward and reload, and the page. When an agent asks to use a
site's signed-in session (`pane.permission_requested` for a web pane, which a `granted` command or
a probe of the profile's cookies raises), the pane shows the same card as the Mac, above the page
(`views/permission`), and answering it lets the waiting command run; the card is part of the window,
so the page is laid out again when it appears or goes. A "Hand back to the agent" button appears
when a person has taken the pane over during a read. The address goes through
the engine (`web.navigate`), which checks it and asks the host to go, as it does for an agent;
the page's address and title go back (`web.host.navigated`) when it has loaded, and `web.navigated`
is applied to the pane. The page reports at most once for the same address and title. Copy, cut,
paste and select all use the window's clipboard (the engine's own clipboard has no window of this
program to ask on Wayland); text composed by an input method is inserted as the page's own
editing. The "+" menu and the Web place open a web pane on Linux; elsewhere they keep saying it is
not available.

## Windows (phase 2)

Settled by ADR 0124 (WebView2, in a window of its own over the pane: a child window does not
show over GPUI's window). What was weighed here, and what phase 2 had to settle, is kept as it was
written:

- Embedding: WebView2 hosts in an `HWND`. A child window over GPUI's window has the same
  limits as the Linux child surface (it covers GPUI's popovers). Composition hosting
  (`ICoreWebView2CompositionController`) draws into a DirectComposition visual and takes input
  through `SendMouseInput`, but needs GPUI's visual tree, which the pinned GPUI does not expose.
  The likely first step is a child window with the pane's rectangle and the web view hidden when
  a menu opens over it, and a measured look at capturing into an image
  (`ICoreWebView2::CapturePreview`) as the Linux pane does.
- Isolated script world: `AddScriptToExecuteOnDocumentCreated` and `ExecuteScript` run in the
  page's world; `ICoreWebView2` has no call that makes another world. The only isolated world
  is the DevTools protocol's `Page.createIsolatedWorld`, through
  `CallDevToolsProtocolMethod`, which is in process (no port, no pipe to another program) but is
  the protocol of automation tools. It may be used only without `Runtime.enable` and the other
  domains whose side effects a page can detect; whether it is acceptable is for the person who
  owns the product to say, and phase 2 will say so before it uses it. The alternative is a
  main-world script that sets no global and patches nothing, which a page that hooks
  `getBoundingClientRect` could see being called.
- Full page and `web.read`: `CapturePreview` takes the viewport, so the walk of this ADR
  (scroll, capture, join, with `contracts/vectors/web-read.json`) carries over unchanged. The
  alternative, `Page.captureScreenshot` with `captureBeyondViewport`, resizes the view during the
  capture, which a page may see as a `resize` event; it is not needed.
- Profile: a `userDataFolder` in the profile directory, with a separate environment for an
  incognito pane (`IsInPrivateModeEnabled`).

The findings of the Windows QA round of the VM were not available while this was written; if
they show a window or input problem of the desktop client that depends on the embedding, they
change the first step above, not the decision for Linux.

## Verification

See the pull request: unit tests of the host, the keys, the policy and the picture; the
integration test of the host over a scripted engine; the real runs on the Ubuntu VM through the
engine's own `web_*` tools; the detection page; and reads of two news sites through two harnesses
running in the pane (`web_open`, `web_read` of 3 to 5 screens, 10 to 25 s each on the
software-rendered VM, summaries correct, no tool other than Soyeht's used).

On a software-rendered screen (the VM) a page with ads or video can keep two web processes and the
window busy enough to be slow; the frame rate is at most 30 a second per pane and only the latest
frame is kept, so a slow window drops frames and does not queue them.

## Consequences

- A web pane works on Linux with the person's own login, and an agent can read it through the
  tools that already exist. The Mac and the desktop are one host protocol.
- The window forwards input to the page, so a feature of a native web view that needs a window
  of its own (a `<select>` popup, a file chooser, a context menu, text composition shown inside
  the page) is not there. Those are listed as follow-ups, with Windows.
- `soyeht-webview` is a new crate in the desktop layer (`scripts/check_layers.py`, AGENTS.md)
  and `libloading` and `image` are new edges of the desktop workspace (both are already in its
  lock file).
- HiDPI follows GDK's scale: the picture carries its device pixels for each logical pixel and the
  window paints it at the logical size. It was not run on a screen of scale 2.
