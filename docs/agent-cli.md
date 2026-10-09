# Agent panes from the CLI

Build the workspace binaries with `cargo build --locked --workspace` from
`engine/`. The examples below address only the new Dev profile. Use a separate
absolute `--data-dir` for disposable tests; do not replace a running profile's
binaries to run a test.

The engine ships a pinned, offline ACP catalogue and merges `harnesses.json`
from its data directory as the custom extension layer. Custom entries contain
`id`, `name`, an executable `command`, `args`, `version`, and optional bounded
environment variables. They cannot use the built-in identifier namespace or
replace `PATH`, `HOME`, `USER`, `LOGNAME`, or profile-owned variables. The
command must resolve to an installed executable. The engine hashes that
executable and combines its reported version and launch configuration when
caching native capability probes. Authentication is handled by the native
session; never put credentials in this file or a command-line argument.

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
soyeht --profile dev pane mode PANE_ID MODE
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
soyeht --profile dev pane auth-cancel PANE_ID
soyeht --profile dev pane logout PANE_ID
```

Cancellation pauses the waiting queue. Sending another prompt keeps that pause;
resume explicitly when ready. Queue and catalog pages expose `nextCursor`, passed
back with `--cursor`. Transcript pages expose `nextCursor`, passed with `--before`
to read older messages. Sign-in method IDs come from the harness catalog. A
terminal sign-in method opens a separate visible terminal pane. On a pane that is
already signed in, `pane auth` signs in again as another account and restarts the
session with its history; `pane auth-cancel` stops a running sign-in and closes its
terminal; `pane logout` asks an agent that supports it to forget its account.

Editor commands use the workspace-relative path only when opening a file. Every
later operation uses the opaque file ID and a revision where the operation must
reject stale input. Lines and UTF-16 columns are zero-based.

```sh
soyeht --profile dev pane create --editor src/main.rs --cwd /absolute/worktree
soyeht --profile dev file open WORKSPACE_ID src/main.rs --line 41 --column 3
soyeht --profile dev file read FILE_ID --revision 7 --from 0 --lines 200
soyeht --profile dev file write FILE_ID 7 41 3 41 3 'inserted text'
soyeht --profile dev file history FILE_ID --limit 100
soyeht --profile dev file revert FILE_ID VERSION_ID 8
soyeht --profile dev file search WORKSPACE_ID needle --scope content --case-sensitive false
soyeht --profile dev file search WORKSPACE_ID old --replacement new
soyeht --profile dev file symbols FILE_ID 9
soyeht --profile dev file diff FILE_ID 9
```

`file write` takes one edit as `START_LINE START_COLUMN END_LINE END_COLUMN
TEXT`. Project replacement uses the same conditional, recoverable engine path
as the Mac client. Add the final `--idempotency-key` suffix when a script may
retry a mutation.

Usage commands return the normalized provider readings. Refresh accepts no
provider for the complete eligible set, or one of the provider IDs returned by
`usage list`.

```sh
soyeht --profile dev usage list
soyeht --profile dev usage refresh
soyeht --profile dev usage refresh codex --idempotency-key usage-refresh-1
```
