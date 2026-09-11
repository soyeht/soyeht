# ADR 0012 · Native client and terminal screen

Status: accepted. Scope: slice 1, Mac app and shared iPhone foundation.

The engine remains the owner of the shell, data and permissions. Clients
must interpret terminal sequences without starting their own processes.
We use only SwiftTerm 1.20.0's `TerminalView`, pinned in the lockfile.
The complete MIT license accompanies the bundle, copied from the dependency
checkout before signing. The project remains Apache-2.0. The dependency plugin
was inspected: it queries its own revision and writes metadata to the build output.

The shared package uses generated protocol types. The Mac socket uses asynchronous
operations; the snapshot replaces the list only after all parts arrive.
Each subscription has a binary reference, generation and offset. A gap appears
in the UI; out-of-order bytes force reconnection. The client repeats the snapshot,
event subscription and tail after reconnecting. Reset clears the interpreter;
an escape sequence may continue in the next frame.

Keyboard input is serialized, limited to 1 MiB and split into 16 KiB requests.
A retry after disconnection retains its idempotency key. Responses to queries
triggered by replay interpretation do not become new input. Clipboard commands
and automatic link opening from output are not executed. Copy and paste initiated
by the user remain available.

Screen 1 supplies colors, relief, radii and proportions. Typography uses rounded
and monospaced system fonts with native scaling and accessibility. The slice
shows only workspaces, shells and pairing, without controls for future features.

The first client test showed that shell adoption changed PID and generation
in the database but published only state fields. `pane.updated` now accompanies
`pane.state_changed` in the same transaction. The snapshot and event-updated list
therefore represent the same pane.

The Debug build accepts `SOYEHT_TEST_SOCKET` only for `engine.sock` in a disposable
`sn-*` directory directly under the system temporary directory. The test checks
that installed profiles and paths outside that directory are refused.
The Release build does not read this variable. This allows testing the window
with a controlled shell without altering the user's startup files.
