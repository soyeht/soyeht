# ADR 0103: The desktop window notifies when an agent needs the person

- Status: accepted
- Date: 2026-10-22

## Context

The Mac raises a system notification when an agent starts waiting for a permission or an
answer and the app is not in front, and a click on it opens that pane. The desktop client
(Linux and Windows, ADR 0084) showed the same requests only as badges inside its window, so a
person working in another window never learned that an agent had stopped.

What the Mac does is exactly this and no more. A request is a pending permission or question of
an agent pane. The first connected view is existing state and announces nothing; a request seen
once is never announced again, so a reconnect is silent. The text is the title "Agent needs your
input" and the line "<pane> · <workspace>". The notification was raised only when the person
had turned it on (off by default) and the app is not active. It is not grouped (each request has its own identifier),
and it does not look at whether the pane is visible, only at whether the app is in front. A
click activates the app and opens the pane when the request is still pending.

## Decision

**One rule, two clients.** The rule is in `contracts/vectors/agent-attention.json`: which
requests the panes hold and in which order, which are new, the text, whether one is raised,
whether they are on by default, and whether a click still has a target. The Mac passes it through `AgentAttentionPolicy` and the
desktop through `soyeht_client::attention`. The Mac's text and its click check now come from the
shared policy instead of the app target. The desktop's badges count questions as well as
permissions, as the Mac's do.

**On by default in both clients.** The owner decided that the Mac and the desktop both notify
until the person turns it off. The Mac keeps its toggle in Preferences (its default moved from
off to on; a person's earlier choice is kept, and the first run asks macOS for permission). The
desktop can be switched off with `SOYEHT_NOTIFICATIONS=off` until it has a settings store.

**The window raises it.** `Workbench` asks `attention::requests` after each batch of engine
events. A new request becomes a notification when GPUI says no window of the application has
keyboard focus (`App::active_window`). The environment variable `SOYEHT_NOTIFICATIONS=off`
turns it off for a run. The desktop has no switch in Preferences: it has no place for settings of
its own yet, and both systems let a person silence an application in their own settings. The
switch follows when such a place exists.

**A seam with one shape.** `os_notify` in `soyeht-desktop` has `show` and `Shown::wait`, the
same on every system. Linux and Windows use the `notify-rust` crate (MIT or Apache-2.0, actively
maintained): on Linux its pure-Rust D-Bus client over `zbus`, which GPUI
already pulls in, and on Windows a toast through `tauri-winrt-notification` and the `windows`
crate GPUI also uses. The crate's default features and its C library backends are off. Any
other system returns `Error::Unsupported`. The seam lives in the desktop crate, not in
`soyeht-platform`: ADR 0086 lets a crate keep a `cfg` module with the same public shape for a
concern specific to its own job, and putting it in the platform crate would make the engine build
a D-Bus client it never uses. A
failure logs its kind only, and a repeat of the same kind is logged once until a notification
succeeds again (ADR 0088).

**A click.** Each notification has a thread that shows it, waits for the click and reports to
the window, which checks that the request is still pending, selects the pane and asks the window
system to bring the window forward. At most 16 notifications wait at once. On Linux each also owns a
D-Bus connection and, with `async-io`, an executor thread, so that is up to about 32 threads. A
slot is released when its thread ends, also when the system call panics (the panic becomes a
failure report). A notification beyond the limit is not raised and is lost, not deferred: its
request was already seen, so it is never retried. The skip is logged once until a slot is taken
again.

## What it does not do

- The window system may refuse to move the window. On GNOME under Wayland it does: GPUI asks
  for an activation token with the serial of its last pointer press, which a click on a banner
  is not, so the shell keeps the window where it is. The click still reaches the window (it logs
  `desktop.notification_clicked`) and selects the pane and focuses its composer, which shows the
  next time the window is in front. The shell sends the banner's own activation token with the
  click, which would raise the window, but GPUI cannot take one. Windows lets a process take the
  foreground only after input it received; a toast click counts as that, but this has not been
  run. GPUI has no taskbar badge, flash or urgency request, so the badges stay inside the window.
- A notification that has left the screen (a Windows toast that moved to the Action Center, or
  a click after the system forgot it) no longer reaches the window.
- On Windows the toast is shown under the PowerShell application identity, the only one that is
  registered without an installer. A packaged build will register its own and set `app_id`.
- The Windows toast has not been run on a Windows machine for this decision; it type-checks
  against `x86_64-pc-windows-msvc`.

## Consequences

`Cargo.lock` of the desktop gains `notify-rust`, `tauri-winrt-notification` and
`windows-version`, and their macOS-only dependencies, which are never built. A change to this dependency
needs a reviewer, as every dependency change does.
