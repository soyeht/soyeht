# Process supervisor

`soyeht-ptyd` holds the pseudoterminal and child process while the engine is
disconnected. The registry lives in `ptyd-registry.json`; retained output stays
in memory until ack, capped at 8 MiB per process. A discarded interval appears
as a gap on the next fetch. The engine owns durable screen recording.

The socket and registry belong to the new profile at
`~/Library/Application Support/Soyeht.next/dev/`. `--data-dir` allows an isolated
directory in tests. The socket path must fit the system limit; tests use
exclusive directories under `/tmp/soyeht-next-test-*`.

```sh
cargo build --manifest-path engine/Cargo.toml -p soyeht-ptyd
cargo test --manifest-path engine/Cargo.toml -p soyeht-ptyd
python3 scripts/install_ptyd_dev.py
```

Installation registers only `com.soyeht.next.dev.ptyd`, with KeepAlive.
It refuses to replace an already loaded job. The installed binary lives in
the profile's `bin` directory, separate from the binary being compiled.

The internal channel uses typed requests in `wire.rs`. The pipes variant is
reserved in the type but refused in this slice. Spawn and close acknowledgements
include registry synchronization. A closed process key stays closed.
An old supervisor is not adopted by PID after restart.

Process tests cover reconnection with the same PID, real PTY size, idempotency,
generation fencing, registry after restart, mutual exclusion and output beyond
retention without a connected engine.
