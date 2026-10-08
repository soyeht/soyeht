# Contract 10 · App bar and subscription usage

Status: CLOSED on 2026-09-11. Design: screens 12 and 12b in
`design/soyeht-desktop.pen`. Colors come from the design tokens and must not
be changed here.

## 1. What it is
A 56 px rail on the left edge of the window, hidden by default. The leftmost
button of the top bar toggles it (`Cmd+Shift+B` does the same). The rail
holds icon-only buttons; the name appears in a tooltip on hover; the active
item has the "well" background. Whether the rail is open is a per-client
preference (contract 07, client preferences), never engine state.

Items, top to bottom: **Agents** (the pane grid of the workspace), **Editor**
(contract 09), **Tasks** (contract 03), **Terminal** (a grid filtered to raw
terminal panes), **iPhone** (paired devices and pairing, contract 06); a
separator; the **usage rings** (§2); a spacer; **Settings** at the bottom.
`Cmd+1` … `Cmd+5` switch views. Switching views is a client-side change:
it never creates, closes or focuses panes in the engine.


## 2. Usage rings
One ring per **provider the user is signed in to on this Mac and for
which a reader exists** (§3), in the order the providers appear in the
harness catalog of contract 02. Screen 12 shows three as an example; the
rail grows with the readers. Claude and Codex show the used fraction of the
weekly window by default, falling back to the current window when no weekly
reading is available. Other providers show their primary reported window.
The provider glyph sits inside the ring; the percentage sits below it. Band
colors use the existing tokens: below 50 % `success`, 50–80 % `warning`
(both ends included), above 80 % `danger`. The rule lives once, in
`UsageBand` (`apps/shared/Sources/SoyehtClient/UsagePresentation.swift`);
the Mac rail, the Mac balloon and Settings, and the iPhone and iPad Settings
all draw through it. A provider that is signed in but has no reader draws
nothing: no ring, no placeholder.

**Ring states** (`UsageEmphasis`):

| State | Engine state | Arc | Percentage | Detail |
| --- | --- | --- | --- | --- |
| Current | `ok`, read in the last 15 minutes | band color, full strength | shown | `updated … ago` |
| Last known | `stale`: older than 15 minutes, restored after a restart and not read again, or kept after a failed refresh | band color of the last reading, muted (45 % opacity) | last value, muted | `as of HH:mm`, and why the latest refresh failed |
| No reading | `unavailable` (including not checked yet), `login_needed`, `no_access` | none; the whole ring is dimmed | "—" | the reason |

A last-known ring never turns gray: it keeps the color band of its value so a
glance still tells green from red, and the muting tells it is old.

Hover opens the detail balloon (screen 12b): provider name, primary window
used and when it resets, secondary window (for example the week) used and
when it resets, where the reading came from and when. Click opens the same
detail in Settings. A provider that needs login shows "sign in in your
terminal" instead of numbers; Soyeht never asks for a password.

## 3. Where the numbers come from (engine, read-only)
Amended on 2026-10-03 by the owner's decision recorded in ADR 0077, which
supersedes ADR 0043 and ADR 0044 for usage readers. The readers follow
CodexBar (github.com/steipete/CodexBar, MIT) source for source.

The engine is the only reader; clients only display and may ask for a manual
refresh. Each provider has one ordered chain of sources, one source per
method behind one trait (`soyeht-usage`); a planner walks the chain until a
source answers. The ring's detail names the source that answered.
- **Claude**: (1) Claude Code's OAuth login, read from
  `~/.claude/.credentials.json` (or `$CLAUDE_CONFIG_DIR`) and, on macOS, the
  `Claude Code-credentials` keychain item, sent to Anthropic's usage
  endpoint; (2) a hidden `claude` session asked for `/usage` in a pseudo
  terminal; (3) on macOS, the claude.ai `sessionKey` cookie from the user's
  browsers, sent to claude.ai's usage API.
- **Codex**: (1) a personal access token in `auth.json`; (2) the ChatGPT
  login in `auth.json` (`$CODEX_HOME`, else `~/.codex`), both sent to the
  ChatGPT usage endpoint; (3) a disposable `codex app-server` asked for
  `account/rateLimits/read`.
- **Cursor**: (1) the Cursor app's own login in its `state.vscdb`; (2) on
  macOS, the cursor.com session cookie from the user's browsers; both sent
  to cursor.com's usage summary.
- **Every other harness**: no ring until a reader exists (unchanged).

Rules every source keeps:
- Read only. Soyeht never refreshes, rewrites or moves another
  application's token, and never stores one. An expired login falls through
  to the provider's own CLI or app server, which refreshes its own login.
- Tokens and cookies travel only to their provider's endpoint and never
  appear in a log, an error, a stored reading or the wire.
- The macOS consent prompts for Claude Code's keychain item and for a
  browser's cookie key are expected; a declined prompt shows "no access" and
  is not asked again by the background for 6 hours.
- The hidden CLI session runs in a scratch directory under the temporary
  directory with tools and MCP servers off, ends within 24 s, and leaves no
  scratch directory or session transcript behind. Claude Code records the
  fixed scratch path once among its projects in `~/.claude.json`; Soyeht does
  not edit that file.
- Linux reads the same files and fallbacks; it has no keychain or browser
  sources (as in CodexBar).

Cadence (engine-owned, ADR 0077): every provider is read at engine start and
then every 5 minutes; after a network failure the next tries come at 15 s,
45 s, 2 min, then 5 min; a provider whose shown window has just reset is
read at once. Background reads skip a source cooling down after a rate limit
(`Retry-After`, else 5 min), a declined prompt (6 h) or a failed app-server
launch (30 min), and reuse a CLI reading for 15 min; a click bypasses all of
them. A failed refresh keeps the last good reading, marked last known, with
the reason the latest refresh failed; the first failure after a current
reading is not shown. The last reading is stored (contract 07) so rings show
a value right after restart, marked stale until refreshed.

## 4. Engine and clients (contract 08)
- `usage.list` → `{providers: [{provider, state: "ok"|"stale"|"login_needed"|
  "no_access"|"unavailable", primary?: {used, resetsAt, label}, secondary?:
  {used, resetsAt, label}, source, fetchedAt}]}`
- `usage.refresh {provider?}` → same shape; rate-limited to one call per
  provider per 30 s (error `RATE_LIMITED` with `retryAfterSec`). It walks the
  chains outside the engine lock, providers in parallel, and may take as long
  as the slowest chain (a keychain prompt, a hidden CLI session).
- `unavailableReason` says why there is no reading or, with a `stale`
  reading, why the latest refresh failed: `not_checked`, `tool_not_found`,
  `app_server_failed`, `invalid_response`, `rate_limited`, `network_error`,
  `login_needed`, `no_access`, `source_failed`.
- Event `usage.changed {provider, ...}` after every manual refresh and
  every background refresh that changed what a ring shows. iPhone reads the
  same method and shows the rings in Settings.

## 5. Out of v1
Cost in currency, per-pane token counts (already delivered by the ACP
`usage` update, contract 04), other providers, a menu-bar or notch widget,
alerts when a window is about to run out.

## 6. Licensing
Provider glyphs are vector icons from an MIT-licensed icon set; the license
notice ships in `apps/shared`. Provider names and marks belong to their
owners and are used only to identify the user's own account.

## 7. Place in the slice plan
Slice 4, together with the editor: the rail is how the user reaches the
editor view. Order inside the slice: rail and view switching → `usage.list`
with the three readers → rings and balloon → Settings detail.
