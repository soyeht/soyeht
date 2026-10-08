# ADR 0077 · Usage readers follow CodexBar's source chains

Date: 2026-10-03
Status: accepted. Supersedes ADR 0043 (usage reader source eligibility) and
ADR 0044 (usage refresh triggers).

## Context

The usage rings were often stale. The Claude ring read only the
`cachedUsageUtilization` entry that Claude Code writes when its own `/usage`
screen runs, so it stayed "last known" for days. The Mac app refreshed only
when the rail opened or the cache file changed, and a provider process ran
only after an explicit click.

ADR 0043 had excluded the stronger sources on purpose. It ruled out Claude
Code's OAuth credential, interactive CLI screens, macOS permission prompts
and spawning a harness to read usage.

On 2026-10-03 the owner decided otherwise, for usage readers only:

- The rings must always be current and refresh on their own.
- They should do it exactly as CodexBar (github.com/steipete/CodexBar, MIT)
  does, with all of its sources in an automatic fallback chain.
- Keychain and browser-cookie prompts, and a hidden CLI session, are
  acceptable. They are the solution, not a problem.

## Decision

### One chain per provider, read by the engine

The engine is the only reader. Clients only display readings and may ask for
a manual refresh.

Each provider has an ordered chain of sources, one per method. All sources
implement one trait (`soyeht-usage`, `sources/`). A planner walks the chain
until a source answers. The chains reproduce CodexBar's automatic mode:

| Provider | Chain, in order |
| --- | --- |
| Claude | OAuth usage endpoint → hidden `claude` `/usage` session → claude.ai browser cookie |
| Codex | personal access token → ChatGPT login in `auth.json` → disposable `codex app-server` |
| Cursor | Cursor app login (`state.vscdb`) → cursor.com browser cookie |

The Claude order follows CodexBar's app planner (`ClaudeSourcePlanner`, auto
mode, app runtime), which is OAuth, then CLI, then web. CodexBar's own
command-line runtime puts the web source first. Soyeht is a resident app, so
it follows the app order.

**Fallback rules.** These are CodexBar's per-source rules:

- A source with nothing to read (no login, cookie or tool) is skipped.
- Codex token sources fall through only on a missing or rejected login. A
  network or server error stops the chain, so a flaky API never starts an
  app server.
- A Cursor session falls through only when it is missing or rejected.
- When every source fails, the error shown is the last one actually
  attempted.

**Credentials stay read-only.**

- The Claude login is read from `~/.claude/.credentials.json`, and on macOS
  also from the `Claude Code-credentials` keychain item.
- Soyeht never refreshes, rewrites or moves another application's token. An
  expired Claude or Codex login falls through to the provider's own CLI or
  app server, which refreshes its own login.
- Tokens and cookies travel only to their provider's endpoint. Credential
  headers reach `curl` on standard input, never on its command line. They
  never appear in a log, an error, a stored reading or the wire.

**The hidden CLI session.**

- It runs `claude` in a pseudo terminal in a scratch directory under the
  temporary directory, with tools disabled and MCP servers off.
- It has a 24-second deadline.
- Afterwards the process group is stopped, the scratch directory removed and
  the session transcript under `~/.claude/projects/` deleted.
- The scratch path is fixed, so Claude Code records it at most once among
  the projects in `~/.claude.json`. That entry is the one trace left, and it
  is Claude Code's own file, which Soyeht does not edit. CodexBar leaves the
  same entry for its probe directory.

**Browsers** are read on macOS only, as in CodexBar:

- Chrome, Arc, Brave, Edge, Vivaldi and Chromium values are decrypted with
  the key the browser keeps in the login keychain. The first read shows the
  macOS consent prompt.
- Firefox cookies are read directly.
- Safari is not read. Its cookie file needs Full Disk Access, which no
  prompt can grant.

**Other applications' databases.** The Cursor state database and the browser
cookie stores are SQLite files. One module, `soyeht-usage/src/foreign_db.rs`,
reads them:

- Each file is opened read-only, with `immutable=1` where its application may
  hold it open. No lock is taken and nothing is written.
- It is the only SQLite use outside `soyeht-store`. The hardening gate's
  `check-no-sql-outside-store` allows exactly that file and its crate manifest
  (`FOREIGN_DATABASE_READERS` in `scripts/hardening_gate.py`). Every other
  path is still checked.
- `soyeht-engine` has no SQLite dependency.

**Linux.** The engine reads the Claude login file, the Codex `auth.json`,
the Cursor state database under `$XDG_CONFIG_HOME` (or `~/.config`), and the
same CLI and app-server fallbacks. Linux has no keychain or browser sources,
which matches CodexBar.

### Cadence and backoff

The engine schedules refreshes itself (CodexBar's refresh timer):

- It reads every provider at startup, then every 5 minutes. That is
  CodexBar's fixed default and its Linux desktop default.
- After a network failure it retries at 15 s, 45 s, 2 min, then every
  5 min.
- When a shown window's reset time passes, that provider is read at once.

Per-source cooldowns protect background refreshes. A click bypasses all of
them.

| Trigger | Cooldown |
| --- | --- |
| HTTP 429 or a CLI rate-limit message | `Retry-After`, or 5 min without one |
| A declined keychain or cookie prompt | 6 h |
| A failed app-server launch | 30 min |
| A successful CLI reading | reused for 15 min instead of starting `claude` again |

Manual refreshes (`usage.refresh`) are still limited to one per provider every
30 s. Background reads do not count against that limit. A click that meets a
background read of the same provider waits 5 s. A batch is all or none, as
ADR 0044 had it. Sources are read outside the engine lock, providers in
parallel.

### Failures keep the last good value

A refresh that fails keeps the last good reading, with its values, source
and age. Its state becomes `stale`, and `unavailableReason` names why the
latest refresh failed (`rate_limited`, `network_error`, `login_needed`,
`no_access`, …).

As in CodexBar, the first failure after a current reading is not shown.
Without any good reading, the failure itself is shown (`unavailable`,
`login_needed` or `no_access`).

The ring's detail names the source that produced the reading.

## Consequences

- The rings stay current with no client involvement. The Mac rail's
  `ClaudeUsageCacheObserver` and its refresh triggers are removed.
- The macOS prompts appear the first time the engine reads Claude Code's
  keychain item or a browser's cookie key.
- `UsageSource` gains `claude_web`, `codex_access_token`, `cursor_app_login`
  and `cursor_browser_cookies`.
  - `claude_desktop_cache` and `cursor_current_period` remain only so that
    stored readings still decode.
  - `UsageUnavailableReason` gains `rate_limited`, `network_error`,
    `login_needed`, `no_access` and `source_failed`.
  - The limit `usageHoverRefreshAfterSec` is removed, and
    `usageOpenRefreshIntervalSec` is replaced by `usageAutoRefreshIntervalSec`.
- Sources are tested with recorded responses through a scripted host. The
  tests use no network or credential.
