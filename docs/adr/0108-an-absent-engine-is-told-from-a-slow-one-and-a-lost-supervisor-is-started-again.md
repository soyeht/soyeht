# ADR 0108: An absent engine is told from a slow one, and a lost supervisor is started again

- Status: accepted
- Date: 2026-10-24

## Context

A QA round of the desktop client on Windows found three faults that share one cause: the
window starts the engine and the supervisor (ADR 0102) and then assumes they stay up.

- After an engine crash, a kill or a reboot, the endpoint file stays and the window never starts
  an engine again. A connection to a closed loopback port is reported as refused by Windows only
  after about 2.1 s, while the client gives a connection 2 s, so it saw `TimedOut`, which the
  window reads as "an engine that is slow", not "no engine".
- Ctrl+C ended nothing in a terminal when the window had started the supervisor. The detached
  start used `CREATE_NEW_PROCESS_GROUP`, which makes a process ignore Ctrl+C, and every shell
  the supervisor started inherited that.
- When only the supervisor died, the engine kept running without it: terminals looked alive,
  took no input, and nothing started the supervisor again until the person killed the engine.

## Decision

**A live listener holds its endpoint file.** On Windows the listener takes an exclusive lock on
one byte far past the end of the endpoint file (the lock the singleton files already use, so the
file stays readable) before it publishes the file, and keeps it as long as it lives. The system
drops the lock when the process ends, however it ends. A client that reads the file and can take
the lock knows that nothing listens, and fails at once with `ConnectionRefused`, the same error
a Unix socket whose listener is gone gives. It does not ask the network. A listener that holds
the lock and does not answer in time is still `TimedOut`: slow, not absent, so the window waits
and does not start a second engine. A process id in the file was rejected (ids are reused after a
reboot, and a stale id of a live process reads as a live engine), and so was a longer connection
timeout (it would delay every other failure and still not tell the two apart).

**No process group for a detached program.** `spawn_detached` no longer passes
`CREATE_NEW_PROCESS_GROUP`. The hidden console it creates already keeps the launcher's Ctrl+C
away. As Unix resets the signals a terminal's keys rely on before it starts a terminal's child,
Windows now restores the default Ctrl+C handling (`SetConsoleCtrlHandler(NULL, FALSE)`) before
it starts a pseudo-console child, so a supervisor that was started in a process group of
someone else's making still gives its terminals Ctrl+C.

**The engine starts the supervisor it loses, when it was told how.** The program that starts the
engine says how the supervisor is started: the window passes `--restart-supervisor <program>`
(`soyeht_protocol::data_directory::RESTART_SUPERVISOR_FLAG`), the supervisor's program next to
the window. When the engine has no link to the supervisor and nothing listens at its endpoint,
it starts that program the way the window does (`process::detached::spawn_service`: the same
data directory, output appended to `logs/ptyd.log`), off the Core lock, at most once in 15
seconds, and then reconnects as it already does, which restores every pane. A supervisor that
answers is never started again. Whoever keeps the supervisor alive owns that rule: the launch
agent on the Mac (`KeepAlive`, which does not pass the option), the engine on Linux and Windows
when the window started it, and the service seam of ADR 0102 once it exists. A person who starts
the engine by hand does not pass it, and the engine only reconnects. The path must be absolute, since a relative one would be read from
wherever the engine runs. A failed start is logged on its first failure and on a change of kind,
not at every attempt.

**Clients are told.** The engine already sends `ptyd.lost` when a request finds the link gone and
`ptyd.reconnected` when it is back. The desktop client now shows "Supervisor disconnected" from
the first until the second, the text a refused request already carries
(`soyeht_protocol::SUPERVISOR_DISCONNECTED`), so the person is told before typing. A window that
connects while the supervisor is down missed `ptyd.lost`, so after each connection it asks
`engine.status` and shows the same line when `ptyd.connected` is false. The Mac client keeps ignoring both events: its supervisor comes back by itself
within a second.

## Consequences

- A person on Windows can start the app a second day: the window finds the stale file, the lock is
  free, and it starts the engine.
- Ctrl+C works in every terminal of a supervisor the window started.
- Killing only the supervisor costs the terminals a few seconds, not the engine.
- The CLI and any other client of the endpoint get `ConnectionRefused` for a stale file at once
  instead of a timeout after 2 s.
- An engine of an earlier build that is still running holds no lock, so a client of this build
  reads its endpoint as stale until it is restarted.
