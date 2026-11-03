# ADR 0135: The login unit leaves the PATH to the engine

- Status: accepted
- Date: 2026-11-03

## Context

A clean `.deb` install on Ubuntu 26.04 arm64 showed that the engine a systemd user unit starts at
login can see different tools than the engine the window starts. The unit written by `soyeht
service install` fixed `PATH` to `/usr/local/bin:/usr/bin:/bin:~/.local/bin:~/.cargo/bin` (ADR
0114), a stand-in for the login shell that systemd does not run.

The engine does not need a stand-in. It asks the person's interactive login shell for the `PATH`
its agents get (`agents::environment`: `bash -l -i -c 'printf "$PATH"'`, remembered in the data
directory and asked again in the background), and falls back to its own `PATH` only when that shell
cannot be asked. The window-started engine and the login-started one run the same probe. What
differs is the `PATH` the shell starts from, because a login shell adds to the one it inherits:

- the window's engine inherits the window's, which the person's session built;
- the unit's engine inherited the fixed list, which holds nothing the session added outside the
  shell's startup files (an `environment.d` entry, a desktop's own folders), and is the whole
  answer when the probe fails (a slow `nvm`, a shell the probe cannot ask).

## Decision

**A systemd user unit names no `PATH`.** The engine and the supervisor start with the environment
of the user's manager, which a desktop session fills from the login environment, so the base the
probe starts from is the one the window has. The shell adds what the person's startup files add,
as it does for the window. Nothing in the unit has to know where `nvm`, `~/.npm-global`, `volta`
or `mise` put a program.

**The Mac keeps its list.** launchd starts a program with almost no environment and no session to
import one from, so the agent still names the system and user tool folders under the shell's
answer (ADR 0120). Only the Linux `LINUX_SYSTEM_PATH` is gone.

**A Windows task already names none.** The task starts with the account's own environment, and the
engine there has no login shell to ask: its launch `PATH` is its own process `PATH` (the machine
and user `PATH` of the registry as they were at logon). A window opened later inherits the
Explorer's, which is the same registry value. A folder added to the user `PATH` after logon (a new
`nvm-windows` install) is in neither until the next logon. This was read in the code and not run
on Windows.

## Consequences

- A unit written before this change keeps its `PATH` line until `soyeht service install` writes
  it again (the seam rewrites a definition whose bytes differ).
- If the probe fails and nothing is remembered, the login-started engine uses the manager's
  `PATH`, which can lack `~/.local/bin` where the old fixed list had it. The engine logs the
  failure by kind (`engine.launch_path_probe_failed`), and a rescan asks the shell again. The
  window-started engine falls back to the window's `PATH` in the same case.
- A start at boot with lingering, before any session, has the manager's default `PATH`; the probe
  supplies the rest.

## Not verified

Windows. The Linux behaviour is checked on Ubuntu 26.04 (arm64): the launch `PATH` the engine
remembers after a login start and after a window start, with a harness under a fake `~/.nvm`.
