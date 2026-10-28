# ADR 0111 · A background usage probe starts nothing a person must answer

Date: 2026-10-24
Status: accepted. Narrows ADR 0077, which accepted a hidden CLI session.

## Context

On Windows 11 a machine that had Claude Code installed but was not signed in
opened a "Sign in - Claude" page in the default browser again and again: about
60 tabs in five hours. The engine's usage schedule starts the hidden
`claude` session every few minutes. A signed-out Claude Code does not show its
usage panel. It shows a first-run setup and then a menu of login methods, and
the session's driver answers a screen it does not know by pressing Enter every
0.8 s. The first Enter after the menu appeared chose a login method, and
Claude Code opened the browser.

ADR 0077 accepted keychain prompts, browser-cookie prompts and a hidden CLI
session, because they make the rings current. It did not accept a background
job that opens a window for the person.

## Decision

A probe is background work. It must never open a browser, show a prompt that a
person has to answer, or wait for input. The engine enforces this in three
places, each one rule in one place.

1. **Nothing starts without a login to use.** The hidden Claude Code session is
   started only when the machine holds a stored Claude login: an access or
   refresh token in Claude Code's credentials file or, on macOS, its keychain
   item. An expired login still counts, because Claude Code refreshes it. With
   none, the source answers "sign in needed" at once and starts no process.
   A declined keychain read answers "no access" and starts none either.
2. **A key is pressed only on a screen the driver knows.** The session's driver
   works from an allow-list. It answers the trust dialog of its own scratch
   directory and cancels, with Escape, the notice that offers a new renderer. It types `/usage` (without Enter) only when the input line of the
   prompt is on screen, and presses Enter only while the typed command is on
   that input line, or on the palette and "press enter" notices it already
   knew. On a blank screen, a menu with new wording or a screen it has never
   seen, it waits, and the 24 s deadline ends the read as a failure that opens
   nothing. A setup or login-method screen it recognises ends the read at once
   with "sign in needed".
3. **Every command the usage host starts goes through one function.**
   `usage/process.rs` `harden` sets `BROWSER` to a program that opens nothing on
   Unix; Windows has no stock program like that and its openers do not read
   `BROWSER`, so only the first two rules protect it. Standard input needs no
   rule: the bounded runner gives none, and the Codex app server and the pseudo
   terminal get a pipe that only the source's own conversation writes to.
   `curl`, `security`, the Codex app server and the Claude Code session all
   begin from it, and they run under the bounded runner or the session's own
   deadline.

The keychain item is asked for at most once per refresh (the context of a read
memoizes it), so the stored-login check never adds a second consent prompt to
the one the OAuth source already caused.

Sessions take turns on their scratch directory. The directory has a fixed name
(Claude Code records the path once among its projects, and a name per run would
add an entry every time), and a session removes and recreates it, which would
pull it from under another session's running child. The other session can be a
refresh of another engine on the machine: the installed app, a development
build, a second profile. So the turn is an operating-system file lock (`flock`
on Unix, `LockFileEx` on Windows) on a file inside a directory beside the
scratch directory, named after it with `-lock`. The session waits for the turn
within its own 24 s. The lock directory and its file stay after a run on
purpose: the lock is not their existence but the hold on the open file, which
the system drops when the holder exits or crashes, so a crashed engine never
blocks the others and the next session reuses the same file. A session that
cannot get the turn is unrun and reads as a failed source.

The scratch directory is made while holding the turn, exclusively and
owner-only (mode 0700): whatever is at the path then (an earlier session's
leftover, or on Linux something another user put at the predictable path of the
shared temporary directory, even a link) is removed, never used, and a
directory or link that appears between the removal and the creation makes the
session unrun instead of running in it. A failure to make or remove the
directory is logged by kind, once per streak.

A session that fails (a screen it does not know, a program that never answers)
is not started again by the background for the longest usage backoff, like a
signed-out answer; a click tries at once.

A signed-out answer backs off. The Claude session source is not asked again by
a background read for 30 minutes (the longest backoff). The engine's usage service keeps a streak per
provider: while a provider stays signed out, the background schedule reads it
after 10 minutes, then 20, then every 30 (the longest backoff), and it logs the
state when it starts and at doubling counts only. Any other answer ends the
streak. A click is never held back and does not count in the streak, so signing
in is noticed at once and a few clicks do not push the schedule out.

## Consequences

- A signed-out machine shows "sign in needed" for Claude and starts no
  `claude` process. The other Claude sources (the OAuth endpoint, the browser
  cookie on macOS) are unchanged.
- A Claude Code that has a login file but needs a person (an expired refresh
  token, a first-run setup) is reported as "sign in needed" instead of being
  driven through its menus.
- The Codex app server never gets a login method to choose: the engine sends
  it only `account/read` and `account/rateLimits/read`. It follows the same
  command rule. Cursor reads files and HTTPS only.
- macOS keychain consent prompts stay as ADR 0077 decided: they are the
  operating system's own prompt, shown once and answered by the person.
- A person who uses Claude Code only through `ANTHROPIC_API_KEY`,
  `CLAUDE_CODE_OAUTH_TOKEN` or a Bedrock or Vertex setup has no stored login, so
  Claude reads "sign in needed" for them. Claude Code has no subscription usage
  panel in those setups, so no reading is lost.
