# Engine storage

`database::Store` opens `soyeht.db`, applies migrations and executes the transaction
for state, events and the idempotent result. The database uses WAL,
synchronous=FULL and foreign keys. The engine is its only writer. Event IDs use
SQLite's positive integer range; terminal offsets use the protocol's u64 domain.

`snapshot` reads state and cursor together. `replay` uses an exclusive cursor and
refuses an old cursor before returning any event. Cleanup keeps at most ten
thousand events or seven days without losing the counter when all old events
are removed. Idempotent results have 24-hour retention.

`segments::Log` maintains the pane's screen. The format begins with `SOYPT001`.
Each record has a u8 type, u64 offset, u32 size, payload and u32 CRC32;
integers are big-endian. Type zero carries bytes and type one carries the end of
a gap as u64. The maximum data per record is 64 KiB. Writes synchronize before returning.

Recovery discards only an incomplete tail in the last file. Checksum corruption,
invalid format or an unmarked gap are errors. Cleanup removes entire old segments
and synchronizes the directory. There is no parallel flat log.

```sh
cargo test --manifest-path engine/Cargo.toml -p soyeht-store
```

Tests inject failure inside a transaction, a full database, a future version,
a truncated tail and checksum corruption. They also verify backup, idempotency,
cursor handling, 512 KiB rotation and 32 MiB retention.
