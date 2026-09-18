# Slice 4 · usage reader investigation

Date: 2026-09-12. Host: Caio's Mac. Profile: `Soyeht.next/dev`.

This report records the source investigation required by contract 10 §3 before
usage rings ship. It contains source names and normalized outcomes only. No
credential, account identifier, current percentage or private response body is
recorded.

## 2026-09-18 Claude correction

The original Claude probe below proved that `/usage` returned the required
windows, but later on-device testing proved that launching it can make macOS
attribute Claude Code's Media Library access to the Soyeht engine. It is no
longer an eligible automatic source.

Claude Code 2.1.275 already writes the same five-hour and seven-day windows,
reset times and `fetchedAtMs` under `cachedUsageUtilization` in
`~/.claude.json`. The production reader now parses only that key, never reads
`accountUuid`, and never launches `claude`. Old cache data remains visible as a
gray last-known value. Selected source label: `claude_desktop_cache`.

The Dev harness registry contained one configured harness, `claude-code`.
Contract 10 also names candidate providers, so the investigation covered every
named candidate that was installed on this Mac. The production registry is
limited by [ADR 0043](../adr/0043-usage-reader-source-eligibility.md): a source
must be bounded, machine-readable and read-only before it can become a reader.

## Results

| Provider | Installed client | Source tested | Real-login result | Slice 4 reader |
| --- | --- | --- | --- | --- |
| Claude | Claude Code 2.1.269 | Non-persistent `/usage` command with an empty MCP configuration | Passed; returned current and weekly windows without an agent turn | Yes |
| Codex | Codex CLI 0.154.0 | Disposable app-server `account/rateLimits/read` request | Passed; returned primary and optional secondary windows | Yes |
| Cursor | Cursor CLI 2026.09.02-c22c1a3 and desktop 3.3.22 | Existing Keychain session sent to the provider's own current-period endpoint | Passed; returned billing window and normalized plan fraction | Yes |
| Gemini | Gemini CLI 0.46.0 | CLI quota path used by `/stats` | Existing login was rejected by the installed client; continuing required authentication state to change | No |
| Copilot | Copilot CLI 1.0.83 | Interactive `/usage` screen | Displayed plan consumption, but only inside a trusted interactive session with no stable read-only output mode | No |
| OpenCode | OpenCode 1.18.30 | `stats --pure --days 1` | Returned local token and cost totals, not a subscription window | No |
| Pi | Pi 0.84.1 | Installed command and local account-state shape | Exposed local session totals, not a subscription window | No |
| Grok | Grok 1.0.13 | Model catalog and command inventory | Exposed models and local disk information, not a subscription window | No |
| omp | Not installed | Command discovery | No local client or login to inspect | No |
| fx | Not installed | Command discovery | No local client or login to inspect | No |
| Amp | Not installed | Command discovery | No local client or login to inspect | No |
| Muse | Not installed | Command discovery | No local client or login to inspect | No |

## Original selected sources

### Claude

The preferred source passed. The probe ran the usage command in print mode with
session persistence disabled, MCP disabled and permission prompts disabled. It
returned JSON with zero agent turns and zero API-processing time. The result
contained the primary session window, weekly windows and their reset times.
The reader can parse the normalized window labels, fractions and reset times
without creating a conversation.

The CLI credential file was inspected by key name only. It contains OAuth and
subscription metadata, so the contract's endpoint fallback is structurally
possible, but it was not invoked because the preferred source worked. The
desktop cache file `plan-usage-history.json` was also present. Its schema stores
timestamped usage samples, but it was not selected or treated as proof of a
current authoritative reading.

Original source label: `claude_cli_usage`; superseded by the correction above.

### Codex

The preferred source passed. A disposable stdio app-server completed
`initialize`, received `initialized`, and answered
`account/rateLimits/read`. The result contained a primary window with used
percentage, duration and reset time, plus an optional secondary window. No
session was created and no token left the process boundary.

The login file was inspected by key name only. The access-token fallback was
not invoked because the app-server source worked.

Selected source label: `codex_app_server`.

### Cursor

The desktop client's installed code identifies its current-period endpoint and
the response fields for billing dates and plan usage. The probe retrieved the
existing access token from the macOS Keychain into process memory, sent it only
to that endpoint, received HTTP 200 and discarded it. The response contained a
billing window and plan fraction. No credential value was printed or written.

The ACP session and `cursor-agent about --format json` do not expose the
subscription window, so neither is a fallback.

Selected source label: `cursor_current_period`.

## Sources that did not qualify

Gemini's installed code has a quota request with remaining fractions and reset
times. On this Mac, however, the installed version rejected the existing login
and required reauthentication or an update. The investigation did neither.
The source therefore cannot satisfy the read-only rule on this installation.

Copilot's interactive `/usage` screen displayed plan usage and a credit window.
The noninteractive print mode treats `/usage` as an agent prompt instead of a
local command. Driving the interactive screen would create session and trust
state and would depend on terminal rendering, so it is not a production reader
interface.

OpenCode reported local activity totals. Pi's installed sources likewise track
session totals and cost. Grok exposed its model catalog but no subscription
window. The remaining named candidates were absent, so no real login existed to
test.

## Implementation consequence

The first ordered registry has three readers: Claude, Codex and Cursor. Claude
uses the passive cache source in the correction above; Codex and Cursor use the
passing sources recorded below it. Fake source ports cover `ok`, `stale`,
`login_needed`, `no_access` and `unavailable` in the local gate. A later reader
for any excluded provider requires a repeatable, noninteractive, non-mutating
real-login probe and a new report entry before a ring is added.
