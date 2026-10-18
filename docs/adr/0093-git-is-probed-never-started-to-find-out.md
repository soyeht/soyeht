# ADR 0093: Git is probed, never started to find out

- Status: accepted
- Date: 2026-10-17

## Context

On a stock Mac, `/usr/bin/git` is a launcher. Without the Command Line Tools it opens the
system "install developer tools" dialog instead of running Git. Creating an editor pane
started it, so a clean machine showed an Apple installer nobody asked for (issue 345).
`command -v git` cannot tell the two apart, and starting the program to find out is the
action to avoid.

## Decision

`soyeht_platform::git::Probe` decides from files, without starting a process: the first
`git` on `PATH` is the one the system would start; when it is the macOS launcher, a `git`
must exist in the developer directory the launcher consults (`DEVELOPER_DIR`, then the
`xcode-select` link, then the Command Line Tools folder). Any other `git`, such as one from a
package manager, needs nothing. Other systems are never probed. The answer is not cached, so
installing Git later takes effect on the next request.

`soyeht_process::run_git` runs the probe before `run`, and `soyeht_process::require_git` is
the same check for a program that starts Git itself (`wt`). Every place the engine reaches
Git goes through one of them: the `soyeht-vcs` command port, the revision snapshot of the
task review, and the worktree commands.

A refused start is `soyeht_process::Error::GitUnavailable`, reported to clients as the wire
error kind `git_unavailable` with one message that says what is missing and that nothing was
installed. Features that do not need Git are untouched.

## Consequences

The clients decide how to present `git_unavailable`: it is a typed state, not a failure to
retry. The presentation (non-modal, with a way to install later by choice) is a product
decision and is proposed separately.
