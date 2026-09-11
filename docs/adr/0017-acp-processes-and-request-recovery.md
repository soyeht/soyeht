# ADR 0017 · ACP processes and request recovery

Status: accepted. Scope: slice 2.

A pipes process does not receive the terminal closure that ends a shell
under a PTY. Stdin EOF can also be ignored. Each harness therefore has a
private guardian, started by the ptyd executable before the harness. The
guardian creates a process session and receives a private control socket.
That descriptor is not passed to the harness.

When ptyd dies, control EOF makes the guardian terminate its own group,
including itself. The group identity remains occupied by the guardian
until that instant; there is no lookup or signaling by a PID saved on disk.
Closing the pane uses the same group. Normal exit preserves the harness
exit code and also terminates children that stayed in the group. This
controls the lifetime of a cooperative process; it does not isolate a
program that deliberately detaches from the group.

The engine remains the sole session owner. The guardian does not understand
ACP, persist state or accept connections. The public registry records the
harness PID; engine reconnection does not replace it. Updating the internal
protocol to v3 requires updating both services together in a profile with
no open panes. An incompatible version is refused without creating another ptyd.

Acknowledging output records the transcript but does not answer a native
call. Harness JSON-RPC requests therefore stay in a separate registry until
a complete response has been written to stdin. Response fragments do not
count. An idempotent write receipt prevents repeating bytes when the engine
crashes before receiving the acknowledgement.

Sequence is the exclusive byte offset, including the newline. The recovery
key is the record start within the process generation. The engine persists
that identity before executing any requested effect. String, number and explicit null IDs
are distinct; an absent ID is a notification. Empty string IDs remain valid; concurrent reuse of the same ID produces an explicit gap,
never silently replacing the previous request.

The request registry holds up to 128 entries and 8 MiB per process, in
addition to the 8 MiB of raw output. Both line parsers use up to 1 MiB each.
Listing returns metadata only; reading a body uses pages up to 64 KiB,
without exceeding the channel's JSON limit. Exceeding any limit or receiving
invalid JSON marks `recovery_gap` until the instance closes. The engine must
not interpret that state as complete recovery.

Stderr goes to a private profile file and never enters ACP stdout.
Diagnostic rotation shares the service-log follow-up in
[issue 10](https://github.com/soyeht/soyeht/issues/10).
