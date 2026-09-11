# ADR 0028 · Put engine rules behind IO ports

Status: accepted for hardening round 1.

The engine defines three ports for the external effects used by its rules:
durable storage, process supervision and wall-clock time. The composition root
opens the real SQLite store, connects the real supervisor client and installs
the system clock. Rules receive only the port surface they need.

The storage port keeps atomic change callbacks. Those callbacks expose the
store's named domain operations and never expose a database connection. This
preserves the existing transaction boundary between durable state and public
events while allowing read-side rules to run against an in-memory test double.

The supervisor port owns request and process-identity exchange. The clock port
owns Unix time, millisecond deadlines and public timestamps. Engine code does
not read wall-clock time directly outside the real clock implementation.

Each rule family has a test that uses the in-memory store, fake supervisor or
fixed clock. These tests open no database, socket or child process. Runtime and
end-to-end tests continue to verify the real adapters and transaction
boundaries.
