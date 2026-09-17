# Mac app in the Dev profile

Install the engine and ptyd following `dev-local.md`. The app exclusively uses
the socket at `~/Library/Application Support/Soyeht.next/dev/engine.sock`.
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

An agent pane is created from the same new-pane sheet by selecting agent mode,
an installed harness, its advertised model and effort, a permission mode and a
working directory. The conversation view renders normalized messages, thoughts,
tool calls, plans, inline terminal results, questions, queue state and confirmed
configuration. See [the real ACP procedure](manual-acceptance-slice-2.md) for the
authenticated adapter and physical-device checks.

Open a terminal by choosing a directory. The list and screen follow the engine.
Use “Add iPhone” to display the QR; compare the fingerprint on the phone before
allowing access. The code expires in ten minutes. Pairing requires the engine
to have a tailnet address; loopback is only for local tests.
