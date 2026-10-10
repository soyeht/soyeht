# ADR 0086: The engine runs on Windows, behind a platform crate

- Status: accepted
- Date: 2026-10-08

## Context

ADR 0085 made the engine run on Linux. Windows differs in more than a few
`cfg` branches: the engine and the supervisor talk over Unix sockets, hold
terminals on `openpty`, keep agent adapters alive with a guardian process and
signals, open files with `O_NOFOLLOW`, and protect data with POSIX modes. None
of these exists on Windows as it is on Unix. A first build on Windows 11 ARM64
failed in nine crates.

The same OS-specific calls also appeared in several crates (the singleton lock
was copied between the engine and the supervisor; owner-only modes and
`O_NOFOLLOW` were repeated in the store, the registry, the observations and the
export). Adding a second implementation of each next to the first would have
doubled the duplication.

## Decision

**A platform crate.** `engine/soyeht-platform` holds the system calls that
differ between macOS, Linux and Windows for four concerns, and has no Soyeht
dependencies (it sits beside `soyeht-protocol`, AGENTS.md says so):

- `local`: the endpoint a process of the same user connects to, with a
  synchronous flavour and a `tokio` flavour.
- `fs`: owner-only files and folders, opens that refuse a link, durable renames,
  the singleton lock, folder links, and a check that a file or folder is
  owner-only.
- `user`: the identity of the user running the process.
- `process`: ending a process together with everything it started (a process
  group on Unix, a job object on Windows) and the Windows launch of a child into
  a job. `soyeht-ptyd` and `soyeht-process` both use it.

Other crates call these and contain no `cfg` for them. Where a crate has an
operating-system difference of its own, it keeps a `cfg` module with the same
public shape on every system (`ptyd` child and terminal, the engine's network
interfaces and log capture).

**The local endpoint on Windows.** The standard library cannot create a Unix
socket on Windows (`windows_unix_domain_sockets` is unstable), and a named pipe
is a plain file handle without the read and write timeouts, `try_clone` and
`shutdown` that the supervisor client, the CLI, the MCP server and the tests
rely on. A loopback TCP connection has all of them, for both the synchronous
and the `tokio` code. So on Windows `engine.sock` and `ptyd.sock` are small
files that only the account can read, holding a loopback port and a random
32-byte secret. The secret is never sent: the two sides exchange nonces and an
HMAC proof in each direction, and the client sends only its nonce until the
listener has proved it knows the secret (contract 08, section 2.5). Only a
process that can read the owner-only file can speak to the engine, which is the
guarantee the Unix socket's mode 0600 gave, and, unlike a secret sent first, a
process that merely listens on the port named by a stale file cannot pass for the
engine. The listening socket is bound with `SO_EXCLUSIVEADDRUSE`, so a second
process cannot share the port. The connection counts as the same user for the
same reason, and, as on Linux, never as a signed UI.

A flood of connections that never answer cannot lock out a good client, but the
mechanism is eviction, not a guarantee: each connection has 2 seconds in all, in
its own thread or task, and at most 256 may be mid-exchange. At the limit the
oldest is dropped to make room for the newest, so a good client is turned away
only if 256 other connections arrive before its exchange (a few milliseconds)
completes, and then it retries. An earlier draft refused new connections at a
fixed limit of 64, which let any local user hold every slot for 2 seconds and
lock the real client out; that is what this replaces. A supervisor that exits
removes its endpoint file as the engine does; a killed one leaves a file that the
next start replaces.

**Terminals and processes.**

- A terminal runs on a Windows pseudo console (ConPTY). The same `Session`
  type as on Unix gives the supervisor and the engine's usage sessions an input
  file, a non-blocking output, a resizer and a child. A read with nothing to
  read fails with `WouldBlock`, as on a Unix master.
- Every child lives in a job object with kill-on-close. Ending the job ends the
  whole tree, and the system ends it if the supervisor itself dies, however it
  dies. That is what the guardian process and the descendant walk do on Unix,
  so there is no guardian on Windows. A terminal's job also ends processes a
  person detached from the shell, which Unix leaves alone.
- The job code (launch into a job, and `Spawned`: wait for the exit, `kill_tree`,
  which also waits until the job is empty, because ending a job is asynchronous) is
  in `soyeht-platform` (`process`). `soyeht-process` uses it to run a bounded command
  in its own job, killed on its deadline, with the engine's environment; a command
  that ends by itself releases its job first, so a background process it left running
  survives as on Unix. On Unix the shared code is `process::signal_group`.
- Environment: a child gets the variables Windows programs need to start
  (`SystemRoot`, `ComSpec`, `APPDATA`, ...) in place of the Unix list.
- The default shell is `cmd.exe`-class on Windows by way of `COMSPEC`; the engine
  does not run a login shell to learn `PATH`, because a Windows process starts with
  the account's `PATH`.

**Secure file access.** Data folders are made private with a protected ACL for
the user and the system, and the engine checks that its observation and export
folders and its push key are owner-only by reading the ACL. Agent file access
cannot be descriptor-relative on Windows; it opens every folder and the file as
the link it may be and refuses links. A folder swapped for a link between two
steps is the one race this cannot close, and the code says so.

**The singleton lock on Windows.** A lock there is mandatory: a locked range
cannot be read by anyone else, so a lock over the whole lock file would make a
project search that walks the data folder fail on it. The lock is one byte far
past the end of the file, which still keeps a second process out.

**What is unsupported on Windows**, each with a typed `Unsupported` result
instead of silence: the list of local servers (`lsof`; another process's
working directory is not readable through a supported interface; for the same
reason cleaning a task worktree stops with an explanation, because it cannot tell
whether a process works inside it), moving a project entry to the trash (already so on Linux), reading the macOS keychain
and browser cookies for usage sources, and the signed-UI check.

**Tests.** Tests that need a POSIX shell, `ps`, `perl`, `chmod` modes or a
shell script as a fake program run on Unix only. The fakes the engine rig
needs are Rust programs (`soyeht-fake-worktrunk`, `soyeht-fake-lsp`,
`soyeht-fake-acp`), so the rig runs on Windows. The supervisor has its own
Windows tests for terminals, pipe processes and jobs. Tests that create symbolic
links need the right to create them (Developer Mode, or an elevated shell);
`scripts/check_engine_windows.ps1` checks that first and says so.

## Consequences

macOS behaviour is unchanged: the same calls run, now through the platform
crate and `cfg` modules that hold the code that was inline. Linux keeps ADR
0085. The workspace builds and passes its tests on Windows 11 ARM64.
`docs/platforms.md` lists what differs and how to run the checks.

## Rejected

- Named pipes for the local endpoint: no timeouts or `try_clone` on a plain
  handle, and a hand-written security descriptor in every server.
- `uds_windows`: a synchronous socket with no `tokio` integration, so the
  engine would still need a second transport.
- A guardian process on Windows: a job with kill-on-close gives the same
  guarantee without a helper that could itself be killed.
- A pseudo-terminal crate: the `Session` seam is the one place the engine needs
  it, and it is small.
