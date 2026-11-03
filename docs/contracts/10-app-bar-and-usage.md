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
The track and the arc share one path, inset by half the widest line
(`UsageRingGeometry`, drawn by `UsageRingGauge`), so no stroke reaches past the
ring's bounds: the rail scrolls and clips at its column, and the rail draws no
scroll indicator over the rings.

**Ring states** (`UsageEmphasis`):

| State | Engine state | Arc | Percentage | Detail |
| --- | --- | --- | --- | --- |
| Current | `ok`, read in the last 15 minutes | band color, full strength | shown | `updated … ago` |
| Last known | `stale`: older than 15 minutes, restored after a restart and not read again, or kept after a failed refresh | band color of the last reading, muted (45 % opacity) | last value, muted | `as of HH:mm`, and why the latest refresh failed |
| No reading | `unavailable` (including not checked yet), `login_needed`, `no_access` | none; the whole ring is dimmed | "—" | the reason |
| Activity only | `ok` or `stale` with an `activity` and no window (OpenCode, §3) | none; the whole ring is dimmed | "—" | what the device recorded, for example `Last 7 days on this device: 1.2M tokens · $3.40`, and that it is not a limit |

An activity-only ring never shows a percentage: there is no plan limit to be a fraction of.

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
  `account/read` with `refreshToken` (so a login the endpoint refused is
  renewed first) and then `account/rateLimits/read`.
- **Cursor**: (1) the Cursor app's own login in its `state.vscdb`; (2) on
  macOS, the cursor.com session cookie from the user's browsers; both sent
  to cursor.com's usage summary.
- **OpenCode**: one meter for a program that talks to many providers. It follows the provider
  OpenCode is set to: the `model` of OpenCode's configuration (`provider/model`), else the
  provider of the model it used last. Each step reads the data of that provider only.
  1. Provider `opencode` or `opencode-go`: the **Go allowance**, from the Go inference API with
     the Console login OpenCode stores.
  2. Provider `zai-coding-plan` or `zhipuai-coding-plan`: the **GLM Coding Plan quota**, from
     the plan's quota API with the token OpenCode stores for the provider.
  3. Any other provider, a provider with no plan, or an account Go does not serve: **activity
     on this device**, read from OpenCode's own database (§3, "OpenCode activity").
- **Every other harness**: no ring until a reader exists (unchanged).

### OpenCode sources and their status

| Source | Wire `source` | Status | Request |
| --- | --- | --- | --- |
| Go allowance | `opencode_go_allowance` | **experimental**: OpenCode's own `UsageLimits` service (anomalyco/opencode pull request 58694, `GET /api/experimental/usage/limits`), not a documented API | `GET {origin}/zen/go/v1/usage`, bearer = the Console login, `x-opencode-org-id` = the login's workspace. The same request, with the same login, read-only; no OpenCode server is started. |
| Z.ai / Zhipu GLM Coding Plan | `opencode_zai_plan` | **vendor plugin**: no documented API; the request is the one Z.ai's own plugin makes (github.com/zai-org/zai-coding-plugins, `plugins/glm-plan-usage/skills/usage-query-skill/scripts/query-usage.mjs`); limit entries read as CodexBar reads them | `GET https://api.z.ai/api/monitor/usage/quota/limit` (`https://open.bigmodel.cn` for `zhipuai-coding-plan`), the plan's token in `Authorization` |
| Device activity | `opencode_local_history` | **local**: OpenCode's database; `opencode stats` prints the same totals as text only (no JSON), so it is not run | no request; the database is opened read-only |

- **Logins.** The Go login is the active `opencode` row of the `credential` table of OpenCode's
  database; a Z.ai login is the row of the provider. When that table holds no credential at all,
  OpenCode keeps its logins in `auth.json` and the provider's entry is used (a Go login there has
  no workspace, so Go is skipped). A table that holds credentials but none for the integration is
  the answer: a leftover `auth.json` is never used in its place. `XDG_DATA_HOME` (absolute),
  `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `OPENCODE_CONFIG` and `OPENCODE_DB` are honoured as
  OpenCode honours them. A configuration file that is not plain JSON is not read; the model
  OpenCode used last decides.
- **Go** maps the five-hour meter to the primary window ("5-hour") and the weekly meter to the
  secondary one ("Weekly"). The monthly meter has no slot on the wire and is not shown. The
  Console named by the login may only be a host of `opencode.ai`: the login goes to no other.
  HTTP 403 (no Go subscription) means nothing to show and leaves the activity; 401 is a rejected
  login and also leaves the activity. A reply of any other shape is `invalid_response`, never a
  number, so a change on the provider's side shows as "usage reader returned invalid data".
- **Z.ai** sorts the plan's token (`TOKENS_LIMIT`, `CREDIT_LIMIT`) limits by window length: the
  shortest is the primary window, the longest the secondary one, labelled "5-hour", "Weekly",
  "3-day" and so on from `unit` and `number`. The MCP lane (`TIME_LIMIT`) is not shown. A reset
  beyond what a five-hour window allows is dropped and the percentage kept. An account with no
  active plan answers `{"code": 500, "msg": "…coding plan", "success": false}`: that is
  `plan_expired` ("the plan has expired or is not active"). The chain ends there: the activity of
  an expired plan's provider would hide it. Set to a plan but with no stored login:
  `login_needed`.
- **OpenCode activity** sums the input, output and reasoning tokens (cache reads and writes are
  not counted) and the cost OpenCode recorded on assistant messages of the last 7 days
  (`usageActivityDays`) of the provider OpenCode is set to, or of every provider when none is
  known. A provider that bills nothing per token records a cost of zero, and no cost is shown.
  The database is opened read-only; like any reader of a write-ahead database, SQLite touches its
  shared-memory index file (`opencode.db-shm`) and nothing else.
  The totals are a count of this device only: another machine's use of the same plan is not
  in them. The Go allowance and the Z.ai quota are the account's, and say so. Only the
  `message` table is read; an OpenCode that keeps its history elsewhere answers `no_data`.
- **States.** OpenCode is not on this machine (no program on `PATH`, no database, no `auth.json`):
  `tool_not_found`, "OpenCode not found on this Mac". Set to a plan and not signed in:
  `login_needed`. Plan expired: `plan_expired`. History with no assistant message in the window:
  an `ok` reading with 0 tokens.

Rules every source keeps:
- Read only. Soyeht never refreshes, rewrites or moves another
  application's token, and never stores one. An expired login falls through
  to the provider's own CLI or app server, which refreshes its own login.
- Tokens and cookies travel only to their provider's endpoint and never
  appear in a log, an error, a stored reading or the wire.
- The macOS consent prompts for Claude Code's keychain item and for a
  browser's cookie key are expected; a declined prompt shows "no access" and
  is not asked again by the background for 6 hours.
- A background read never starts anything a person must answer (ADR 0111):
  no browser, no prompt, no stdin. The hidden CLI session starts only when
  Claude Code holds a stored login (an expired one counts), presses a key only on
  a screen it knows (an unknown screen is waited out, never answered), and a signed-out answer is not
  retried by the background for 30 min. A provider that stays signed out is
  read after 10 min, 20 min, then every 30 min, until it signs in; a click
  always reads at once.
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
  {used, resetsAt, label}, activity?: {label, tokens, costUsd?}, source, fetchedAt}]}`
- `activity` is what a provider with no plan limit recorded on this device: a `label` that
  names the span, a token count and, when the provider bills per token, the cost in US dollars.
  It is a count, not a fraction: a reading with an `activity` and no window shows no percentage.
  Both clients write it the same way (`activity` cases of
  `contracts/vectors/usage-presentation.json`): tokens as an exact count under 1,000, then
  one decimal with `K`, `M` or `B`; the cost with two decimals and a `$`; joined by " · ".
- `resetsAt` is ISO 8601 text in UTC (`2026-10-11T09:00:00Z`), or empty when the provider
  gave no reset time. A provider that reports Unix seconds (the Codex app server sends them as
  a string) is converted by the engine's reader, so a client reads no other form. The detail
  says "Resets in 1h 30m" for a future reset, "Reset due" when the time is not after now (the
  reading predates the reset and the next refresh brings the new window) and "Reset
  unavailable" when the text is empty or is not a timestamp. Both clients pass the `resets`
  cases of `contracts/vectors/usage-presentation.json`.
- `usage.refresh {provider?}` → same shape; rate-limited to one call per
  provider per 30 s (error `RATE_LIMITED` with `retryAfterSec`). It walks the
  chains outside the engine lock, providers in parallel, and may take as long
  as the slowest chain (a keychain prompt, a hidden CLI session).
- `unavailableReason` says why there is no reading or, with a `stale`
  reading, why the latest refresh failed: `not_checked`, `no_data`, `tool_not_found`,
  `app_server_failed`, `invalid_response`, `rate_limited`, `network_error`,
  `login_needed`, `no_access`, `source_failed`, `plan_expired`. `no_data` means the provider
  answered with a well-formed reply that reports no window yet; it is not
  `invalid_response`, which is a reply of the wrong shape.
- `plan_expired` means the provider answered that the account has no active plan; the
  detail says to renew it. The ring is "No reading" until it does.
- Event `usage.changed {provider, ...}` after every manual refresh and
  every background refresh that changed what a ring shows. iPhone reads the
  same method and shows the rings in Settings.

## 5. Out of v1
Cost in currency for the plans that have a window (the OpenCode activity shows the cost OpenCode
recorded, and only there), the Console budgets of OpenCode and the monthly Go meter,
per-pane token counts (already delivered by the ACP `usage` update, contract 04), other providers, a menu-bar or notch widget,
alerts when a window is about to run out.

## 6. Licensing
Provider glyphs are vector icons from an MIT-licensed icon set; the license
notice ships in `apps/shared`. Provider names and marks belong to their
owners and are used only to identify the user's own account.

## 7. Place in the slice plan
Slice 4, together with the editor: the rail is how the user reaches the
editor view. Order inside the slice: rail and view switching → `usage.list`
with the three readers → rings and balloon → Settings detail.
