# ADR 0124: The desktop web pane on Windows is WebView2, in a window of its own over the pane

- Status: accepted
- Date: 2026-10-30

## Context

ADR 0110 gave the desktop client a web pane on Linux (WebKitGTK, drawn off screen) and left
Windows as "phase 2": the pane said it was not available. The goal is the same as on the Mac and
on Linux: a person opens a site in a pane with their own login, and an agent reads the page (a
full-page picture and the text) through the engine's `web.command`. No automation tool, no debugging
port, no spoofed browser, and our scripts in a world the page cannot see.

Windows 11 ships Microsoft Edge WebView2 (the Evergreen runtime, Chromium). This ADR settles what
ADR 0110 left to phase 2: how the runtime is loaded, how a page lives in a GPUI window, which thread
owns it, where its data lives, what happens without the runtime, and what a page may do. The host
protocol and `soyeht_client::web_host` do not change.

## Decision

### How the runtime is loaded: `webview2-com`, whose static loader finds the runtime

Four ways were weighed.

| | Ships a DLL | Public API | Notes |
| --- | --- | --- | --- |
| `webview2-com` (COM interfaces and Microsoft's loader linked into the program) | no | yes | three new crates; the loader is Microsoft's own, linked statically |
| `WebView2Loader.dll` loaded at run time | yes, next to the program | yes | an installer and an updater must carry a native DLL |
| `EmbeddedBrowserWebView.dll` of the installed runtime, called directly | no | no: an undocumented export that the loader itself calls | breaks without notice |
| COM registration of the system | | | there is none: WebView2 is not a registered COM class |

`webview2-com` is taken. `webview2-com-sys` carries `WebView2LoaderStatic.lib` for x64, arm64 and
x86 and links it statically on the MSVC targets, so the program starts on a machine with no runtime
(the loader fails only when a view is first asked for) and no DLL is shipped. The loader looks for
the runtime by Microsoft's documented rules. The Linux crate loads its libraries at run time because
a build host has no WebKitGTK development files; here nothing is needed at build time (the library
is in the crate) and nothing at start, so the same reason does not ask for run-time loading.

New dependencies, for the required review: `webview2-com`, `webview2-com-sys` and
`webview2-com-macros` (MIT). Everything they use is in the lock file already: `windows` 0.62
(GPUI uses it), `windows-core`, `windows-implement`. The desktop gains an edge to
`raw-window-handle` (GPUI's own dependency) to read the handle of its window. `soyeht-webview` keeps
no Soyeht dependency and the layering is unchanged.

### The page is a window of its own, owned by the program's window

The first plan of ADR 0110 was a child window of GPUI's window. It does not show: GPUI composes its
window with DirectComposition and creates the target as topmost
(`CreateTargetForHwnd(hwnd, true)` in `gpui_windows`), so what GPUI draws is above every child
window. A child window of another process, created over the GPUI window of the program on the VM
(Windows 11 ARM64), was invisible. So the page is a borderless top-level tool window (no task-bar
button, none in the switcher) owned by the program's window, with the system web view as its child:

- An owned window stays above its owner and is hidden with it when it is minimized, and the system
  destroys it with the owner.
- `WebView::place(Placement)` puts it over the pane. `Placement` is the pane's whole area (the page
  lays out for it), the part of it the window shows (`clip`, the content mask of GPUI at paint
  time: a pane scrolled half out of the stage shows half, and the page does not draw outside what the
  program shows of it), the scale, and whether it shows. The arithmetic is `placement.rs` and is
  tested on every system.
- The window the page sits over moving or changing its size moves the page: a hook on the window's
  location changes (`SetWinEventHook`, delivered to the web view thread), so it follows within a
  frame, without the interface thread waiting on it.
- The window hides the page when a menu or popover is open (GPUI's popovers would be below it), while
  a divider or a pane is dragged, and when the pane is not in the frame (another workspace, another
  place, the pane scrolled out): `Workbench` collects the web panes each frame draws and the
  registry hides the rest. A hidden page does not draw. For a picture of a hidden page, the engine's
  capture shows it off screen for the length of the capture (a capture of a page that does not show
  never answers, which was measured) and puts it back.
- Until the page has drawn it shows the face color of the pane (`ViewOptions::background`), so a
  new page does not flash white; a minimized window does not move its pages, since it reports a
  place no screen has.
- Alt+F4 on the page's window closes the program's window.

This costs what ADR 0110's table said of a second window: it is a window to activate (a click on the
page makes it the active window, and the program's title bar then shows as inactive), and a
rounded pane card has a square page over it. The input of the person goes to the page itself (the
engine's own focus, caret, IME, scroll and select lists), so the Windows page needs none of the
forwarding of Linux: `WebView::pointer`, `key`, `insert_text`, `select_all` and `delete_selection`
do nothing there, `take_frame` is `None`, and `soyeht_webview::embedded()` tells the window which of
the two it is dealing with.

Composition hosting (`ICoreWebView2CompositionController`) would draw into a visual of ours, but
GPUI's visual tree is not reachable in the pinned revision, and a capture loop of
`CapturePreview` frames is too slow to be a view.

### Threads

WebView2 wants a single-threaded apartment and a message loop on the thread that makes the
environment and the controller, and calls its handlers back through that loop. GPUI's loop is not
used: GPUI belongs to the program's main thread and hands out no hook for foreign messages.
`soyeht-webview` runs one thread of its own, like the Linux crate: it initializes COM as an
apartment, makes a message-only window to receive jobs, registers the host window class and runs
`GetMessageW`. A job is a closure posted to that window (`PostMessageW`, so it still arrives inside
a nested wait). The `WebView` the rest of the program holds is `Send + Sync` and only posts jobs or
waits for an answer with a timeout; every COM object lives and dies on the web view thread, and the
handlers (navigation, title, source, history, new window, permission, dialog, download, process
failure) run there and update shared state under a lock and tell the window through the sink, which
must not block, as on Linux. Making an environment or a controller waits on that thread with a
pump and a deadline (30 s), never borrowing the table across the wait.

The host window and the program's window are on different threads, which only matters for owner and
child windows: an owner on another thread is allowed, and the input queues are not attached (they
are for a child window of another thread's parent). Nothing on the interface thread waits for the web
view thread except the calls that ask for an answer, which the host (not the interface) makes.

The minimum runtime is the one that has `ICoreWebView2_18` (`LaunchingExternalUriScheme`), Microsoft
Edge WebView2 Runtime 119 or later; the Evergreen runtime of Windows 11 updates itself well past
it. A view on an older runtime fails closed: it is not made, the pane says the runtime lacks what
keeps a page in it, and nothing is shown that the runtime would let leave the pane.

### The user data folder

The page keeps its cookies, storage and cache in `web` under the directory the window passes as the
profile: `<engine data directory>/browser/web`, where the engine's data directory is the
profile's (`Soyeht.next/dev` or `Soyeht.next`, so it contains "next", ADR 0112) and is not ours to
compute here. The crate takes the path and does not know profiles. One environment is made per
folder and shared by its views.

A private (incognito) pane has no profile. Its page is an InPrivate controller on an environment of
its own, in a folder of its own: `<profile>/private/<pid>-<count>`, made new for it (`create_dir`,
which fails when the name is taken, so a folder somebody else made is never used) and refused when
the `private` folder or the new one is a reparse point. An environment of its own matters:
InPrivate pages of one environment share the session a private browser keeps, and one private pane
must not see another's cookies. Nothing the person does is kept, but the browser's own files are,
so the first private page of a run removes the folders of programs that have ended (the number says
whose; a program that cannot be looked at, or whose folder is a link, is left alone).

The environment's options turn off single sign-on with the person's Windows account and leave
everything else at the runtime's own values. The loader reads overrides from outside the program,
so the program refuses to open a web view while any is set (the notice says which kind and how to proceed: unset the variable, or ask the administrator who set the policy): the environment variables
`WEBVIEW2_BROWSER_EXECUTABLE_FOLDER`, `WEBVIEW2_USER_DATA_FOLDER`,
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` (which can open a debugging port), `WEBVIEW2_CHANNEL_SEARCH_KIND`,
`WEBVIEW2_RELEASE_CHANNELS`, `WEBVIEW2_RELEASE_CHANNEL_PREFERENCE`,
`WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER` and `WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER`, and, in `HKLM` and
`HKCU`, any value under `Software\Policies\Microsoft\Edge\WebView2\` in the keys
`BrowserExecutableFolder`, `UserDataFolder`, `ReleaseChannels`, `ChannelSearchKind` and
`AdditionalBrowserArguments` (the list is Microsoft's documentation of
`CreateCoreWebView2EnvironmentWithOptions`). A variable that is set, even to nothing, refuses. Not
covered: a policy of the machine that the loader does not document, and `COREWEBVIEW2_MAX_INSTANCES`,
which only bounds how many old runtimes may stay.

### Without the runtime

`Backend::get()` asks for the runtime's version when its thread starts and fails with
`Error::Unavailable("the Microsoft Edge WebView2 runtime is not installed")`. That answer, and the
refusal of an override, are kept for the run; a start that only ran out of time is not (the next
call waits for the same start, it does not make another thread). The registry keeps a definite
error, the pane shows "Web panes are not available here: ..." and the log has
`desktop.web_view_unavailable` with the error, which is a kind of failure, never the system's
message. A runtime that exists but cannot start is `Unavailable` too, named by the code of the
loader (runtime not found, profile in use with other settings, access refused). A view that cannot
be made does not leave a host window behind, and a view that was made after its caller gave up (the
wait of 70 s covers the two steps of 30 s) is closed, not kept. A message loop that has ended
fails every waiting and later call with `Closed`. Nothing panics, and no call blocks the interface.

### What a page can and cannot do

The rules of ADR 0110 apply as they are; on this engine:

- No host objects: `AreHostObjectsAllowed` is off, web messages are off, and nothing is added to
  the page (no initialization script, no handler, no global). Developer tools, the default context
  menu, the status bar, the browser accelerator keys (print, save, find, F12), zoom control, pinch
  zoom, swipe navigation, autofill and password autosave are off, each where the runtime has the
  setting.
- Permissions (camera, microphone, position, notifications, clipboard, anything) are denied, as on
  Linux: the pane asks the person nothing yet.
- A download is cancelled. A new window is opened in the same page when its address is `http` or
  `https`. Navigation of the page is limited to `http`, `https`, `about:blank` and `blob:`
  (`policy::allows`, shared with Linux); `file:` and the rest are cancelled. A frame in the page may
  load those and also `data:` and `about:srcdoc` (`policy::allows_frame`), as in a browser, which
  refuses `data:` only at the top level: many real pages embed their own such frames. WebView2 tells
  a frame from the top level, so each event has its own rule; WebKitGTK does not (ADR 0110: the
  decision lets a frame's address through and a top level that commits as `data:` or
  `about:srcdoc` is sent back to the page before it, or becomes `about:blank` when it is the
  first; ADR 0110 records the symbols of the installed WebKitGTK that were read to find no
  main-frame signal).
  A link to a program of the system (the runtime's external-scheme launch) is cancelled. A runtime
  that has none of these events (frame navigation, download, external scheme) is not used: a view
  there fails with `Unavailable`, since a page could do what is refused here.
- A script dialog is closed: an alert is accepted and a question answered "no", leaving a page is
  allowed. The runtime's own dialogs are off.
- Not the same as Linux: a file chooser of the page opens the runtime's own dialog, since WebView2
  has no event to intercept it. It needs the person's click on the page (the agent's host makes no
  click in this phase) and the file is the person's choice; it is listed as a difference.
- The page's user agent is the runtime's own (Edge's), not changed.

### The agent reader

Everything of `soyeht_client::web_host` is as on Linux: `snapshot`, `read`, `screenshot`, `navigate`,
`scroll` and `wait` work, and `click`, `type`, `press`, `hover`, `select`, `evaluate`, `console`,
`network` and `resize` answer `internal_error`, because the host answers them and does not ask the
view.

- Pictures: `ICoreWebView2::CapturePreview` as PNG into a memory stream, read back as pixels. It
  takes the viewport, so the walk of ADR 0110 (scroll, capture, join,
  `contracts/vectors/web-read.json`) carries over unchanged.
- Scripts run in an isolated world. WebView2 has no call that makes one; the engine's own protocol
  does, in the process, through `CallDevToolsProtocolMethod`: no port, no pipe, no other program.
  The reader makes four calls and no others: `Page.getFrameTree`, `Page.createIsolatedWorld` (the
  same world each time, so what a script keeps in it lasts until the page changes),
  `Runtime.evaluate` in that world (by value, after a promise settles), and `Page.reload` with the
  cache ignored. No domain is switched on (`Runtime.enable` and the like are the calls a page can
  detect and are never made), so the page sees nothing: a global of the world is undefined to the
  page and the page's globals are undefined to the world, as on Linux. A unit test of the crate
  holds the call sites to this list. **This is the decision that is the owner's, and it is taken
  here for the reader to work**: ADR 0110 said phase 2 would say before using the protocol. The
  alternative is a script in the page's own world that sets no global, which a page that hooks
  what the script calls can see; changing to it is the one module `world.rs` and
  `webview2/script.rs`.
- Scrolling the page for a read is the page's own scrolling (`scrollBy` of the nearest element that
  scrolls, or of the page, from the world): the page sees scroll events and no wheel or key event, as
  on the Mac (ADR 0109). A wheel message posted to the page's window was tried and dropped: the
  engine routes it by where the person's own pointer is, so it scrolled or not by where the cursor
  happened to be.
- A person's pointer, wheel and keys in the pane are the page's own, so the window does not see
  them as it does on Linux. The page's window tells it instead (`ViewUpdate::Person`) when the page
  takes the keyboard for a reason of the person's (a click on it, not the focus the window asked
  for a moment ago) and when a key is pressed in it; the pane is then the person's until "Hand back
  to the agent", as on Linux. A wheel turned over a page that already has the keyboard is not seen:
  that is a known gap.

### Focus, scale and the window closing

The pane taking the keyboard focus gives it to the page (`MoveFocus` and the page window in front),
but only while the program is the foreground application: a page never takes the keyboard from
another program. The person clicking the page takes it by themselves. The page follows the monitor's
scale by itself; the window gives device pixels from GPUI's scale factor. Closing the window destroys
the owned page windows with it; a pane closed or a surface dropped hides its page, and dropping the
view closes the controller and destroys the host window on the web view thread.

## Verification

See the pull request: unit tests of the placement arithmetic, the script answers, the pictures and
the policy; `cargo clippy` for `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`; and runs on the
Windows 11 ARM64 VM through the probe (`examples/probe.rs`) and the program itself: a real page
loads, the title, the address and the history reach the pane, a script runs in the isolated world, a
hidden page is captured, the engine's own reader reads a page through the host protocol, and the
window is resized, dragged, covered by a menu and closed.

## Consequences

- A web pane works on Windows with the person's own login, and an agent reads it through the tools
  that already exist.
- The page is above GPUI's drawing while it shows, so the window hides it under anything it draws
  over the pane; a tooltip over a page is below it.
- Three crates are new to the desktop's lock file, and the owner has to decide on the use of the
  engine's protocol for the reader's world.
- Not done: the program's own shortcuts while the page has the keyboard (the page's window gets the
  keys), a rounded edge for the page, the pane's focus following a click on the page, and a run on a
  screen of scale above 1 (the VM has 1).
