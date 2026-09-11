# Agent panes from the CLI

Build the workspace binaries with `cargo build --locked --workspace` from
`engine/`. The examples below address only the new Dev profile. Use a separate
absolute `--data-dir` for disposable tests; do not replace a running profile's
binaries to run a test.

The engine reads `harnesses.json` from its data directory. Entries contain `id`,
`name`, an executable `command`, `args`, and `version`. The command must resolve
to an installed executable. The engine hashes that executable and combines its
reported version and launch configuration when caching native capability probes.
Authentication is handled by the native session; never put credentials in this
file or a command-line argument.

```sh
soyeht --profile dev harness list
soyeht --profile dev pane create --agent HARNESS --cwd /absolute/worktree --model MODEL --effort EFFORT --permission ask
soyeht --profile dev pane list
soyeht --profile dev model list HARNESS --pane PANE_ID
soyeht --profile dev pane prompt PANE_ID 'Explain the current directory' --idempotency-key request-1
```

Creation returns a pane ID before initialization and the disposable probes are
finished. Initial model, effort and permission selections are applied before a
queued prompt can run. A selection the native session cannot confirm produces
a visible configuration failure and holds the queue.

Results are JSON. A mutation accepts a final `--idempotency-key KEY` suffix.
Reuse the same key and exactly the same arguments after an uncertain connection
failure; changing the request under that key is an error. Without this suffix,
the CLI allocates a fresh key and retains it across its connection retries.

```sh
soyeht --profile dev pane config PANE_ID model MODEL
soyeht --profile dev pane config PANE_ID effort EFFORT
soyeht --profile dev pane yolo PANE_ID on
soyeht --profile dev pane yolo PANE_ID off
soyeht --profile dev pane permission PANE_ID allow
soyeht --profile dev pane permission PANE_ID deny --request REQUEST_ID
```

Configuration returns only after native confirmation or an error. Boolean
options accept `true` or `false`; all other option values remain literal strings.
Permission commands choose the native one-time option. If several permissions
are pending, select one explicitly with `--request`; the CLI never picks an
arbitrary request or changes an elicitation form into a permission answer.

```sh
soyeht --profile dev pane cancel PANE_ID
soyeht --profile dev pane queue PANE_ID
soyeht --profile dev pane queue-remove PANE_ID PROMPT_ID
soyeht --profile dev pane queue-resume PANE_ID
soyeht --profile dev pane transcript PANE_ID --limit 100
soyeht --profile dev pane transcript PANE_ID --before CURSOR --limit 100
soyeht --profile dev pane auth PANE_ID AUTH_METHOD_ID
```

Cancellation pauses the waiting queue. Sending another prompt keeps that pause;
resume explicitly when ready. Queue and catalog pages expose `nextCursor`, passed
back with `--cursor`. Transcript pages expose `nextCursor`, passed with `--before`
to read older messages. Sign-in method IDs come from the harness catalog. A
terminal sign-in method opens a separate visible terminal pane.
