# ADR 0007 — Terminal input retries

Status: accepted for slice 1.

Typing changes an external process. A database transaction cannot undo that change. The engine first records an intent with the supervisor and process identity. The intent stores a cryptographic digest of the request, never the typed bytes.

The supervisor retains the write result by operation identity until the engine confirms that its own receipt is durable. Retrying the request against the same supervisor returns the original accepted byte count. A partial write returns its count; the client sends only the remainder under another identity.

If the supervisor dies between the write and the durable receipt, the result becomes unknown. The engine refuses to retry that operation in a replacement shell with `resync_required`. This conservative interpretation avoids executing a command twice when its result can no longer be queried. The user may send new input after checking the screen.

The supervisor accepts at most 1,024 pending receipts. At that limit, it refuses new writes before touching the terminal; acknowledgements free space. Restarting only the engine preserves the receipts and the process.
