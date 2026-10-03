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
| Last known | `stale`: older than 15 minutes, or restored after a restart and not read again | band color of the last reading, muted (45 % opacity) | last value, muted | `as of HH:mm` |
| No reading | `unavailable` (including not checked yet), `login_needed`, `no_access` | none; the whole ring is dimmed | "—" | the reason |

A last-known ring never turns gray: it keeps the color band of its value so a
glance still tells green from red, and the muting tells it is old.

Hover opens the detail balloon (screen 12b): provider name, primary window
used and when it resets, secondary window (for example the week) used and
when it resets, where the reading came from and when. Click opens the same
detail in Settings. A provider that needs login shows "sign in in your
terminal" instead of numbers; Soyeht never asks for a password.

## 3. Where the numbers come from (engine, read-only)
Readers are per provider and live in one registry in the engine; adding a
provider means adding one reader. Every reader follows the same rules: it
reads only what the provider's own tools already keep on this Mac, never
stores provider credentials, never refreshes or rotates a token, and never
sends a token anywhere except to that provider's own usage endpoint. Each
reader has a **preferred source** and **fallbacks**, tried in order; the
balloon shows which one answered. Verified sources are recorded in
`docs/reports/`.
- **Claude**: the `cachedUsageUtilization` entry already written in
  `~/.claude.json` by Claude Code. The reader parses that key once and ignores
  account identifiers and every unrelated key. Soyeht never launches `claude`
  to obtain usage and never reads or reuses its OAuth credential.
  *Absent vs malformed:* a JSON object without the key is an unpopulated cache
  and shows ordinary "unavailable". Anything else that cannot be read is
  "invalid data": a top-level value that is not an object, or a present key
  with a missing, partial or mistyped `fetchedAtMs` or `utilization`.
  *Tradeoff:* a missing key looks the same as Claude Code renaming the key. A
  rename would therefore show "unavailable" indefinitely rather than "invalid
  data", even after Claude Code has fetched usage. If the ring stays
  unavailable on a Mac where Claude Code shows usage, check the key name first.
  *When the entry exists:* Claude Code writes `cachedUsageUtilization` only
  after it fetches the account's usage itself, which its `/usage` screen does;
  ordinary turns, in the terminal or through the ACP registry adapter, do not
  refresh it, and the install method (native, npm or the adapter's bundled
  CLI) does not change where it lives. A Mac where `/usage` was never opened
  shows "—" with that explanation, and an old entry shows as last known. The
  file is Claude Code's whole configuration, so the reader bounds it at 32 MiB
  rather than the 1 MiB protocol frame; a missing file reports "not found".
  Each window (`five_hour`, `seven_day`) must be present; a `null` window, or
  one whose `utilization` or `resets_at` is `null`, is a window Claude Code
  reports as not running and is left out, so the other window still shows.
- **Codex**: (1) a disposable `codex app-server` asked for
  `account/rateLimits/read` (the same channel the harness already uses in
  contract 04); (2) the access token in the Codex login file sent to the
  account's usage endpoint.
- **Cursor**: (1) the session token the Cursor app keeps in the login
  keychain, sent to the account's usage endpoint (one macOS consent dialog;
  if declined, "no access" until Settings re-enables it). The Cursor ACP
  session reports no usage at all.
- **Every other harness in the catalog** (Gemini CLI, Grok, OpenCode, Pi,
  omp, fx, Copilot and the rest): the slice investigates whether the CLI or
  its local files expose a subscription window. Where they do, a reader is
  added under the same rules; where they do not, no ring is drawn. The
  result of that investigation, provider by provider, goes to
  `docs/reports/` before the rings ship.
- Opening the rail reads Claude's passive cache once. While the rail remains
  open, a file observer rereads it only when that file changes: no polling and
  no hover refresh. A reader that must start a provider process runs only after
  an explicit user refresh. Exponential backoff (up to 30 min) applies after a
  429 or network error. The last reading is stored (contract 07) so rings show
  a value right after restart, marked stale until refreshed.

## 4. Engine and clients (contract 08)
- `usage.list` → `{providers: [{provider, state: "ok"|"stale"|"login_needed"|
  "no_access"|"unavailable", primary?: {used, resetsAt, label}, secondary?:
  {used, resetsAt, label}, source, fetchedAt}]}`
- `usage.refresh {provider?}` → same shape; rate-limited to one call per
  provider per 30 s (error `RATE_LIMITED` with `retryAfterSec`).
- Event `usage.changed {provider, ...}` after every successful or failed
  refresh. iPhone reads the same method and shows the rings in Settings.

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
