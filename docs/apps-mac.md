# Mac app in the Dev profile

Install the engine and ptyd following `dev-local.md`. The app exclusively uses
the socket at `~/Library/Application Support/Soyeht.next/dev/engine.sock` (the production app, whose bundle identifier is `com.soyeht.next.mac`, uses `~/Library/Application Support/Soyeht.next/engine.sock`).
Closing the window does not close the shell. Closing a pane requires confirmation.

Unsigned build, also used in CI:

```sh
bash scripts/check_mac.sh
```

Local build with automatic development signing:

```sh
SOYEHT_DEVELOPMENT_TEAM=YOUR_TEAM bash scripts/check_mac.sh --sign
open '.cache/mac-derived/Build/Products/Debug/Soyeht Dev.app'
```

The development account and certificate must be configured in Xcode.
No distribution profile or certificate is used. The project is
`apps/mac/SoyehtMac.xcodeproj`, scheme `SoyehtMac`.

Client tests use disposable processes and never the installed socket:

```sh
(cd engine && cargo build --locked --workspace)
swift test --package-path apps/shared
```

Workspace layout follows one invariant: populated operational content starts at
the top leading edge. Empty states expand across the whole available workspace
and center within that surface; they do not inherit a content column's width.

The workspace tabs in the top bar scroll sideways like any Apple scroll view:
trackpad swipe, Shift+wheel or a plain mouse wheel. A thin overlay indicator
appears only while the strip moves, and each edge that hides tabs shows a fade
with a chevron that pages one view. Selecting a workspace scrolls its tab fully
into view; scrolled away by hand, the active tab stays pinned at the edge it
left from. "+N" lists the workspaces whose tabs are hidden. The rules live in
`WorkspaceTabStripPolicy` (apps/shared).

An agent pane's composer follows the approved design (`DCzvR` in
design/soyeht-desktop.pen): the message line, then one row of "+" (attach),
the harness chip ("Claude ▾", accessibility label "Switch harness"), the
model, effort and permission menus, the context ring, the mic and send. The
harness chip opens the switch popover: installed harnesses first, then model
and effort, then the button that restarts the conversation on the chosen
harness. Model, effort and permission come from the pane's ACP options and
show only when the harness advertises them (`AgentComposerControls`,
apps/shared). While the agent runs, send becomes stop, the placeholder reads
"Queue a message for …" and Return queues. The mic dictates with Apple's
Speech framework, on the device when possible; the microphone and speech
recognition prompts appear on its first tap. The iPhone and iPad show the same
composer; on a narrow width the chips scroll sideways. The context ring shows for every
agent session: an empty track and a dash until the harness reports its context use (ACP
`usage_update`, sent after a turn), then the share in use. Menu items carry a title only; a
choice's description is its tooltip. The model chip uses the catalogue's model name
(`model.list`) when the client has it.

The agent pane's header shows the state dot, the name, one ticker line with
what is happening (plan step, latest tool calls, session title and the running
turn's time) and the status word. The ticker rolls left when it does not fit,
pauses under the pointer and rotates entries instead with Reduce Motion. Its
content comes from an `AgentTickerSource` in the environment, the ACP source
today. The loop and lead controls use a timer icon, and every header icon has a
tooltip and an accessibility label from `PaneHeaderControls` (apps/shared).

An agent pane is created from the same new-pane sheet by selecting agent mode,
an installed harness, its advertised model and effort, a permission mode and a
working directory. The conversation view renders normalized messages, thoughts,
tool calls, plans, inline terminal results, questions, queue state and confirmed
configuration. See [the real ACP procedure](manual-acceptance-slice-2.md) for the
authenticated adapter and physical-device checks.

Open a terminal by choosing a directory. The list and screen follow the engine.
Use “Add iPhone” to display the QR; compare the fingerprint on the phone before
allowing access. The code expires in ten minutes. Pairing requires the engine
to have a tailnet address; loopback is only for local tests. Without one, the
pairing sheet says “Not reachable from your devices” and why, and it refreshes
the QR when the address appears.
