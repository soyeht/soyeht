# ADR 0109: An agent reads a web pane through the view, and the page sees nothing of the app

- Status: accepted
- Date: 2026-10-24

## Context

A person signs in to a site in the web pane (their own timeline, for example) and asks an agent to
read it. Many sites refuse a known automation tool, so the pane is a real browser with the person's
own session: the system web view, our own code, no known automation tool or protocol (no
browser-driver framework, no remote-automation service, no debugging port). It is not a stealth
tool: it does not fake a fingerprint and it does not touch a captcha, and its only change to the
user agent is the Safari tokens the owner approved (below). The only goal is that the app's own
machinery is not something the page can see.

Before this decision a page could see our code, and the agent had no way to read a page that is
longer than the pane.

### What a page could observe, before

Every visit, any site:

- A user script in the page world at document start. It replaced `console.debug/log/info/warn/error`
  and `fetch` with arrow functions and `XMLHttpRequest.prototype.open/send` with function
  expressions, so `Function.prototype.toString` on each returned our source, and `console.log`
  was no longer a native function.
- Two page-world globals: `__soyehtObservation`, and expando properties on every
  `XMLHttpRequest` instance (`__soyehtMethod`, `__soyehtURL`), plus a `loadend` listener on each.
- A page-world script message handler, `window.webkit.messageHandlers.soyeht_<random>`, so
  `window.webkit` existed and listed a handler.
- Work on every call (`JSON.stringify` of the arguments of each `console` call, a `postMessage`
  per request), so timings of those calls changed.

While an agent works:

- `web.wait` ran a script in the page world every 50 ms. It called `document.querySelector` and
  read `innerText` through the page's own prototypes, so a page that wrapped them received the
  agent's selector and text, and it read `__soyehtObservation`.
- `web.screenshot` with `fullPage` ran a script in the page world to measure the document, and then
  asked the view for a snapshot taller than the view. Measured on macOS 26.4, such a snapshot
  paints only the first view height; the rest of the image is white.
- `web.click`, `type`, `press`, `hover`, `select`, `scroll` and the snapshot already ran in an
  isolated world (`com.soyeht.web.structured`), which a page cannot see. The actions dispatch DOM
  events that are not trusted, which is what an action is.
- `web.evaluate` runs the agent's code in the page world: it is the point of the call.
- Pick element (person-initiated) ran in the page world: a `__soyehtPicker` global, capture-phase
  listeners on `document`, an inline `outline` style written to the hovered element, and a
  page-world call every 80 ms.

The user agent was the bare `WKWebView` value, without the `Version/` and `Safari/` tokens (see
the decision below). Not visible to a page: the `NSEvent` monitor that notices a person's input,
`isInspectable` (the Develop menu can attach; no port is opened).

## Decision

**One isolated world for every script of ours.** `WebPageWorld.isolated` (`com.soyeht.web`) is
where the snapshot, the actions, the wait, the picker and the read run. An isolated world has its
own globals and function wrappers over the same document, so a page sees no global, handler or
patched function of ours, and a page that patched `querySelector` is not called by us. The page world
is kept for three things, each named:

1. `web.evaluate`: the agent asked for code in the page itself, behind its own permission.
2. Console and network capture (`WebPageObservation`). Capture needs the page's own `console`,
   `fetch` and `XMLHttpRequest`, which no isolated world and no public WebKit API can see. It is
   therefore installed for a navigation only when the target is a development host
   (`WebObservationPolicy`: `localhost`, `*.localhost`, `*.local`, loopback and private IPv4 ranges,
   `::1`), which are the pages the person is building, and it is taken off (user script and handler
   removed) in the policy callback of the next navigation to any other site. Console and network
   readings of a site that belongs to someone else are therefore empty. `__soyehtObservation` and
   the XHR expandos are gone; the capture keeps its state in its closure.
3. The component probe of Pick element (React, Vue and Svelte keep their records as properties of the
   page's own DOM objects). It is one read per picked element, on a development host only, and it
   writes nothing.

`web.wait` with `networkIdle` no longer counts requests in flight (that needed the patched `fetch`);
it uses the document's ready state and the end of the last resource on the performance timeline.

**One read path, `web.read`.** The contract gains `web.read {paneId, screens?}` and
`WebReadResult {navigationGeneration, title, url, screens, text, links, endOfPage, truncated,
image}` (protocol first, regenerated). The image goes through the same attachment as
`web.screenshot`. The MCP tool is `web_read`. `web.screenshot` with `fullPage` now uses the same
capture, without the text.

The Mac host runs `WebPageReader` (apps/shared, so `swift test` covers it):

1. Record where the person is (the scroll offset of the scroller), then scroll to the top with the
   view's own `scrollToBeginningOfDocument`.
2. For each screen: wait until the scroll and the content it loaded stop moving; read the text and
   links of what is on screen with one isolated-world script that only reads; take the view's own
   snapshot of the scroller's box; scroll down with the view's own `scrollPageDown`, the action of
   a space bar press. The page sees scroll events from the native scroller. No DOM event is
   dispatched, no node is created and no global is left. A read emits scroll events only, with no
   `keydown`, `wheel` or pointer event, which a person's scrolling would also produce; the
   detector test asserts exactly that. A script that treats "scroll with no input" as automation
   can see it, and removing it would take synthetic input, which this project does not use.
3. When the document does not scroll (`overflow: hidden` on the root) but one element covering
   half of the view does, that element is the scroller: the same loop runs on its box and
   advances it with `scrollBy` from the isolated world, because a native page-down cannot reach it.
4. Fixed and sticky bands: the script measures the height of what stays fixed at the top and
   bottom of the scroller. The native page-down leaves a joint of about 40 points, so when the
   bands are taller than that the extra is scrolled back with the same relative `scrollBy`, and
   the stitch keeps the top band only of the first screen and the bottom band only of the last.
5. Stop at the end of the page, at the limit, at the deadline (80% of the command timeout), or
   when a person takes control (`human_took_over`, checked before every scroll). If the pane is
   resized during the read it fails, since screens of two sizes cannot be joined.
6. Join the snapshots by the offsets the page reported. Each later snapshot adds only the rows
   below what the earlier ones cover. The joined picture is drawn at no more than 4 000 000 pixels
   and then shrunk by the existing PNG bound to `webScreenshotBytes`; it is never allocated at
   full size.
7. Put the person's place back with `scrollTo` from the isolated world, and wait for it to settle,
   on success and on any failure. If a person took over, the page is left alone. A snapshot, a
   click or a rect read after a read therefore sees a still page at the place the person left.

The answer is measured as JSON will carry it (escaping included) and the last links, then the last
lines of text, are dropped until it fits `webReadBytes`, with `truncated` set; a long page never
fails the read for size. The page script also marks `truncated` when it stops at its cap of 40 000
text nodes.

Text is the visible text of each screen grouped by block, read once (a block seen again at the same
page position, or a pinned header with the same words, is kept once), so a virtualized timeline that
unmounts what scrolled away is still read. Links are the visible `http(s)` anchors with their text.

Limits are protocol limits: `webReadScreens` (12, the most a call may ask for),
`webReadScreensDefault` (4), `webReadTextBytes` (64 KiB), `webReadLinks` (100), `webReadBytes`
(512 KiB, the response budget of the JSON part). When a limit cuts the text or the links the
result says `truncated`; when the read stops before the page ends, `endOfPage` is false and
`screens` says how far it got.

### Why the screens are stitched

Measured on macOS 26.4 with a 20-block, 6000 px page in a 800 x 600 view that is not on screen:

| Method | Result |
| --- | --- |
| `takeSnapshot` with a rect taller than the view | image of the asked size, only the first 600 px painted, the rest white (blocks 2 to 19 white) |
| `createPDF` | one 783 x 6000 page in 16 ms, all 20 blocks correct for static content; vector print layout (width differs from the view), no lazy or virtualized content, fixed and sticky elements as printed, not as seen |
| `scrollPageDown` + `takeSnapshot` of the viewport, stitched | what the person sees, including content that loads on scroll; every block of a 3040 px page at its own row; 6 screens in 4.5 to 5.3 s on a host at load average 80 (about 0.8 s a screen, mostly the wait for the scroll to settle); 95 KB as PNG |

The stitch is the only one that gives a correct picture of an endless page, and it uses no API
newer than macOS 11 (`callAsyncJavaScript`). `scrollWheel` events built from an unposted `CGEvent`
also scroll, but their location is outside the window and they stop short; `scrollPageDown` is the
view's own and exact.

### What a page can observe, after

- A visit to any site that is not a development host: nothing of ours in its world. No user script,
  no handler (`window.webkit` is as in a bare view), no patched native, no global, no node, no
  attribute, no listener.
- While an agent reads (`web.read`, `web.snapshot`, `web.wait`, `web.screenshot`): the same, plus
  scroll events and no other (no `keydown`, `wheel`, pointer or focus event), one document read per
  screen on the page's main thread (isolated-world script time, bounded to 40 000 text nodes), and
  a page that moves while it is read and moves back to where the person had it.
- Still visible by design: the actions of an agent (untrusted DOM events), `web.evaluate`, the
  capture on a development host, and, while a person picks an element, a highlight (an inline
  `outline` on the hovered element) and capture-phase listeners from the isolated world.
- The user agent names Safari (decided by the owner on 2026-10-25). The `WKWebView` default has no
  `Version/` and `Safari/` tokens, and sites treat a WebKit view without them as an embedded
  browser (Google sign-in refuses those). The pane sets `applicationNameForUserAgent` to
  `Version/<v> Safari/605.1.15`, where `<v>` is the `CFBundleShortVersionString` of the Safari
  installed on this Mac, read at run time, and the major version that ships with the system when
  it cannot be read. The rule is `SafariIdentity` in `apps/shared`, a pure function with tests. It
  is the same WebKit engine Safari uses and claims no other browser; nothing else about the page's
  fingerprint is changed. iOS and iPadOS render no web pane, so there is no Mobile Safari form.

## Consequences

- Tests: `WebPageDetectorTests` load a page that, from its first line, checks its own
  world (globals against a pristine iframe, `webkit.messageHandlers`, `Function.prototype.toString`
  of 15 natives, a `MutationObserver` over the document, and the `isTrusted` of every event) while
  the snapshot, wait and read run. It finds nothing on another site and sees scroll events only,
  finds the capture on a development host (the control that proves the detector works), and finds
  nothing after leaving one.
- The Mac app keeps its thin views: `WebPageObservation`, `WebStructuredActions`, `WebPageReader`,
  the policy and the stitch moved to `apps/shared`, where `swift test` runs them against a real
  `WKWebView`.
- The status bar console counts and the agent's `web_console` and `web_network` are empty for a
  site that is not a development host: leaving a development host clears the readings and reports
  zero counts.
- The capture's message handler is registered for the page world of every frame, so a frame of a
  development page can see that it exists. WebKit has no main-frame-only handler; messages from a
  subframe are ignored.
- `web.wait` with `networkIdle` holds when the document is complete and no resource has finished for
  500 ms. A request still in flight is not seen, and the resource timeline stops recording when its
  buffer is full, so a very busy page can read as idle early.
- Native page-down and top-of-document act on the focused scroller first in WebKit. If the
  person's focus is inside a scrolling element, the document measurement and the scroll can differ;
  the loop then stops when the offset stops advancing and reports `endOfPage` false.
- A page that never answers a script leaves the host waiting after the engine's timeout; the read's
  own deadline is checked between screens only.
