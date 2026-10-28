# Process supervisor

`soyeht-ptyd` holds the pseudoterminal and child process while the engine
is disconnected. The registry lives in `ptyd-registry.json`; retained output
stays in memory until ack, capped at 8 MiB per process. A discarded interval
appears as a gap on the next read. The engine owns durable screen recording.

The socket and registry belong to the profile's data directory: by default the Dev profile,
`~/Library/Application Support/Soyeht.next/dev/` on macOS (`SOYEHT_PROFILE=prod` selects the
production profile; native locations on Windows and Linux are in ADR 0112). `--data-dir` allows an
isolated test directory. The socket path must fit the system limit; tests
use unique directories under `/tmp/soyeht-next-test-*`.

```sh
cargo build --manifest-path engine/Cargo.toml -p soyeht-ptyd
cargo test --manifest-path engine/Cargo.toml -p soyeht-ptyd
uv run python scripts/install_ptyd_dev.py
```

Installation registers only `com.soyeht.next.dev.ptyd`, with KeepAlive.
It refuses to replace an already loaded job. The installed binary stays
in the profile's `bin` directory, separate from the binary being built.

The internal channel uses typed requests in `wire.rs`, version 5.
`pipes_acp` uses separate stdin and stdout, without echo or newline conversion.
Stderr goes to `harness-logs/<instanceId>.stderr.log`, with permission 0600.
Diagnostic log rotation follows the operational work in
[issue 10](https://github.com/soyeht/soyeht/issues/10).
Spawn and close acknowledgements include registry synchronization. A closed
process key stays closed. An old supervisor is not adopted by PID after restart.

Process tests cover reconnection with the same PID, actual PTY size,
idempotency, generation fencing, registry after restart, mutual exclusion
and output beyond retention without a connected engine.

A private guardian from the same executable ties the pipes process group
to the ptyd lifetime. Killing the engine preserves the harness PID;
killing ptyd closes the private control channel and ends the group, even
if the harness ignores stdin EOF. The guardian stores no session state
and has no public socket. The published PID belongs to the harness.

`pending_requests` lists native requests still awaiting an answer, even
after their bytes have been acknowledged. `pending_request_chunk` reads
each body in chunks up to 64 KiB. The list holds up to 128 requests and
8 MiB in total, separately from output retention. An ACP message is capped
at 1 MiB. Overflow, an invalid message or concurrent reuse of a native ID
marks `recovery_gap`; the engine must report the gap. Only a complete
JSON-RPC response actually written to stdin removes the request. Repeated
writes use the `operation_key` receipt, as with the terminal.

`pipes_lsp` uses the same guarded pipe ownership and raw byte retention, but
does not parse application messages or retain native requests. The engine owns
language-server framing and correlation. Reconnecting the engine adopts a live
language server by instance identity; an exited instance remains inspectable
until the engine closes it and asks the supervisor to start its replacement.
