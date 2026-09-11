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
One ring per subscription the user has signed in to on this Mac, in this
order: Claude, Codex, Cursor. Each ring shows the used fraction of the
provider's **primary window** (the window the provider itself calls the
current one: the 5-hour session for Claude and Codex, the billing month for
Cursor). The provider glyph sits inside the ring; the percentage sits below
it. Band colors use the existing tokens: below 50 % `success`, 50–80 %
`warning`, above 80 % `danger`. A provider with no reading yet, or a reading
older than 15 minutes, draws a dimmed ring with "—".

Hover opens the detail balloon (screen 12b): provider name, primary window
used and when it resets, secondary window (the week for Claude and Codex)
used and when it resets, where the reading came from and when. Click opens
the same detail in Settings. A provider that needs login shows "sign in in
your terminal" instead of numbers; Soyeht never asks for a password.

## 3. Where the numbers come from (engine, read-only)
The engine reads what the provider's own tools already keep on this Mac. It
never stores provider credentials, never refreshes or rotates a token, and
never sends a token anywhere except to that provider's own usage endpoint.
- **Claude**: run the Claude Code CLI's usage command in a non-persistent,
  no-MCP session and parse its output. Fallback when the CLI prints no
  windows: the usage entry in the Claude Desktop HTTP cache, when present.
- **Codex**: the access token in the Codex login file under the Codex home
  directory, sent to the account's usage endpoint. An expired token means
  "sign in", never a refresh.
- **Cursor**: the session token the Cursor app keeps in the login keychain,
  sent to the account's usage endpoint. Reading the keychain item triggers
  one macOS consent dialog per Soyeht build; if the user declines, the ring
  shows "no access" and is never asked again until Settings re-enables it.
- Refresh every 5 minutes while the rail is open; on hover when the reading
  is older than 60 s; exponential backoff (up to 30 min) after a 429 or a
  network error. The last reading is stored (contract 07) so rings show a
  value right after restart, marked stale until refreshed.

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
