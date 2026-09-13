# Human mode spike

Date: 2026-09-13  
Environment: macOS 26.4, Safari 26.4, Dev profile only

## Result

The host-process design is **red**. A public macOS event path can satisfy the
page-facing checks, or it can leave the user's visible pointer alone, but it
cannot do both for an out-of-process `WKWebView`.

The selected fallback is a macOS virtual machine with its own browser and
input device. Human mode will not be implemented in the host Mac web pane.
The structured browser tools and ordinary user-driven web pane can proceed.
Human mode remains disabled until the same spike, including the signed-in site
run, passes inside the virtual machine boundary.

## Reproduction

Run:

```sh
scripts/human-mode-spike/run.sh /tmp/human-mode-spike-results
```

The runner builds a small WebKit host and a separate event poster. It executes
the same local page twice. The `process` run sends events only to the host
process. The `global` control sends them through the system event tap and
restores the pointer afterward. The page contains a button, an input and a
muted local video. No remote page, account or credential is involved.

## Measurements

| Check | Process-directed path | Global control |
| --- | --- | --- |
| DOM event is trusted | Red: no DOM event arrived | Green: `isTrusted=true` |
| Pointer move precedes click | Red: no DOM event arrived | Green |
| Click opens a popup | Red | Green |
| Click starts muted video | Red | Green |
| User agent matches running Safari | Green | Green |
| No automation flag is active | Green: `navigator.webdriver=false` | Green |
| Keystroke intervals vary | Red: no key event arrived | Green |
| Visible pointer stays at the user's location | Green | Red: system delivery moves it before restoration |

The popup and video are the operative user-activation checks. WebKit reported
`navigator.userActivation.isActive=false` after the handler, but both guarded
actions succeeded during the trusted click.

The process-directed path was also tried from the host process itself and from
a separate helper process. Both produced the same result: the `WKWebView`
received no pointer or keyboard DOM events. This is consistent with page input
being handled outside the host process.

## External signed-in site

Not run on the host path. The isolated WebKit profile has no authorized
signed-in site, and importing another browser's cookies is forbidden. More
fundamentally, the host path had already failed the trusted-event and pointer
independence combination. The external read-only, two-minute feed check is a
required part of the virtual-machine spike before human mode can be enabled.

## Decision

System-wide input is rejected for the product because it competes with the
person's mouse and keyboard. Hiding, moving and restoring the pointer would
only conceal that interference. Process-directed events are rejected because
they do not reach the page. Script injection and automation-driver paths are
rejected because they do not meet the trusted-event contract.

A virtual machine provides a separate input device and browser process. It can
preserve trusted events and user activation without taking over the host
pointer. This fallback is outside the host web-pane implementation and must be
gated by a new objective before it is built.
