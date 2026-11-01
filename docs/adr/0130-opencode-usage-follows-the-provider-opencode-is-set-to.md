# ADR 0130 · The OpenCode meter follows the provider OpenCode is set to

Date: 2026-11-01
Status: accepted. Extends ADR 0077 (usage source chains) with a fourth provider.

## Context

Claude, Codex and Cursor each have one subscription, and the meter reads its window. OpenCode is a
program that talks to many providers. The owner set it up with a GLM Coding Plan (Z.ai), which has
expired; the same OpenCode also holds logins for other plans and for providers that bill per
token. One ring for "OpenCode" has to say something true for whichever provider OpenCode is set to,
and must not turn a count of tokens into a percentage of a limit nobody reported.

## Decision

**The meter follows the provider OpenCode is set to**: the `model` of OpenCode's configuration
(`provider/model`), else the provider of the model it used last (`model.json` in its state
directory). Each source reads the data of that provider only. The sources, their order and their
status are in contract 10 section 3.

| Provider OpenCode is set to | Source | Status |
| --- | --- | --- |
| `opencode`, `opencode-go` | Go allowance: `GET /zen/go/v1/usage` | experimental: OpenCode's own `UsageLimits` service (anomalyco/opencode pull request 58694) |
| `zai-coding-plan`, `zhipuai-coding-plan` | GLM Coding Plan quota: `GET /api/monitor/usage/quota/limit` | vendor plugin: Z.ai's own plugin, no documented API |
| any other, or no plan to read | tokens and cost OpenCode recorded on this device | local: OpenCode's database; `opencode stats` has no JSON output |

**Activity is not a quota.** A provider with no plan limit gets a reading with an `activity`
(`label`, `tokens`, `costUsd?`) and no window. The wire carries a count, not a fraction, so no
client can show a percentage for it: the ring is dimmed with a dash and the detail shows the count.
The `used` of a window stays a fraction of a reported limit, and only that.

**Expired is a state.** The quota endpoint answers an account with no active plan with HTTP 200 and
`{"code": 500, "success": false}` and a message that names the coding plan. That is
`plan_expired`, a new `unavailableReason`; the chain ends there, because the device activity of an
expired plan's provider would hide the expiry. "Not signed in" (`login_needed`) and "no OpenCode on
this machine" (`tool_not_found`) are the existing states.

**Read-only, and the login goes to one place.** A login is read from OpenCode's credential table
(its active row) or, when that table holds no credential, from `auth.json`. Nothing is refreshed,
rewritten or stored. The Go login goes only to a host of opencode.ai, whatever the credential says,
and the Z.ai token only to `api.z.ai` or `open.bigmodel.cn`. OpenCode's database is opened
read-only (`foreign_db.rs`, the one SQLite use outside the store, ADR 0077) and through the session
index, because the `message` table of a heavy user is large.

**A changed reply is "invalid data".** Both HTTP sources read an exact shape. Anything else is
`invalid_response`, never a number. The Go endpoint is read from OpenCode's source and is marked
experimental because it may change; the quota endpoint is read from the vendor's plugin.

## Consequences

- `UsageProvider` gains `opencode`; `UsageSource` gains `opencode_go_allowance`,
  `opencode_zai_plan` and `opencode_local_history`; `UsageUnavailableReason` gains `plan_expired`;
  `UsageReading` and `usage.changed` gain `activity`. The stored readings table (schema 32) accepts
  the new provider.
- The ring follows the weekly window for OpenCode as for Claude and Codex.
- The monthly Go meter and OpenCode's Console budgets have no slot on the wire and are not shown.
- A Go login kept only in `auth.json` has no workspace, which the Go request needs, so Go is skipped
  and the activity shows.
- The device activity counts this device only; the Go and Z.ai windows are the account's.
- The `message` table is the only history read. A layout without it answers `no_data`.
