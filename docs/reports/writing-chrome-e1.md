# Writing chrome E1 validation

Date: 2026-09-20

Build under test: `f42536f` (`codex/writing-chrome-e1`)

Product: rewrite, bundle `com.soyeht.next.dev.mac`

Machine: VM1, macOS 26.4, Retina 2×. The signed app was installed at
`/Applications/Soyeht Dev.app`; the engine and PTY daemon remained the rewrite
services under `~/Library/Application Support/Soyeht.next/dev/bin`.

This report covers the first writing-chrome slice: the pure P5/P2 policy and
metrics, the text-pane implementation, the common 40 pt header, pane-halo
clearance, and overlay scrollers for all three writing kinds and for the
minimum-size overflow scroll view. Screenplay and presentation consume the
common scrollbar rule in this slice; their header/menu migrations remain later
slices.

## Accepted measurements

Every number below comes from `PaneLayoutContentMetrics` through
`WritingChromeMetrics.standard`; the SwiftUI views do not repeat the literals.

| Element | Measurement | Token / rendering |
|---|---:|---|
| common header | 40 pt high | `face` background |
| header horizontal inset | 18 pt | both edges |
| file name | Nunito 14/600 | `ink-secondary` |
| word count | Nunito 14/400 | `ink-muted` |
| header action gap | 14 pt | named metric |
| options glyph | 16 pt in 32 pt target | `ink-secondary` |
| selection toolbar | 232×32 pt | capsule, `face` |
| toolbar icon | 16 pt, 28 pt target | `ink` |
| toolbar rhythm | 32 pt step | 28 pt target + 4 pt gap |
| toolbar horizontal inset | 6 pt | both edges |
| toolbar-to-selection gap | 8 pt | policy chooses above, then below |
| toolbar shadow | y 4, blur 12 | `shadow-dark` at 40% |
| toolbar safe halo | 16 pt | clamps to pane content |
| dimmed header | opacity 0.35 | restored by pointer/interactive state |

Color samples are the accepted design tokens: `face #E8EDF4`,
`ink #3E4A66`, `ink-secondary #6E7A96`, `ink-muted #8E9AB3`,
`selection #C9D6EE`, and `accent #5B7CFA`.

## Action inventory

No action from the previous fixed toolbar disappeared.

| Door | Actions |
|---|---|
| selection toolbar | Bold, Italic, Heading 1–3, bulleted/numbered list, Quote, Link, Image |
| `Writing options` menu | font, reading/full width, Focus mode, split columns, split rows, close pane |
| existing keyboard doors | Command-B, Command-I and Command-K remain wired |

Link wraps the selected text and Image uses the selected text as alt text. Both
open their small destination popover only while a real selection exists; there
is no modal “requires selected text” failure. A clean writing pane closes
directly. A dirty document uses writing-specific copy: `Close <file name>?` and
`Unsaved changes will be lost.`

The formatting surface is exposed as the `Formatting selection` accessibility
container; every action and the `Writing options menu` has a named AX control.

## Pure-policy proof

`WritingChromePolicyTests` asserts:

- the complete action inventory for text, screenplay and presentation;
- the accepted P5/P2 measurements above;
- toolbar placement above/below a selection and 16 pt halo clamping;
- typing dims chrome while selection, menus, popovers, hover and pane drag keep
  it visible;
- clean versus dirty writing close behavior.

The focused shared suite passed 67/67. The full shared suite passed 232/232,
and both unsigned and signed runs of `scripts/check_mac.sh` completed with
`BUILD SUCCEEDED`.

## VM1 visual and interaction proof

| Width | Result | Evidence under `~/evidence-e1` on VM1 |
|---:|---|---|
| 1024 | 66 ch centered; 40 pt header; complete toolbar; options menu and every action reachable | `E1-selection-1024.png`, `E1-options-1024.png` |
| 700 | same tone/rhythm; prose reflows; selection toolbar stays inside the pane | `E1-selection-700.png` |
| 375 | header title/count/menu remain legible; toolbar clamps without clipping; pane halo remains complete | `E1-selection-375.png`, `E1-options-375.png` |

The manual fixture exercised Bold, Italic, Heading, List, Quote, Link and Image,
then restored the file after every mutation. It also exercised all three fonts,
Focus mode, full/reading width, split columns, split rows and cancellation of
both split choosers. Closing an autosaved document closed it directly; the
otherwise transient dirty branch is covered by the policy test above. Typing
dimmed the header and pointer movement restored it (`E1-header-dimmed.png` and
`E1-header-restored.png`). The fixture ended byte-identical to its backup:
`e2e310c5672a9b6adcfac6cc7df32904dc221a3559bb1ecc3dd037561db3b297`.
The database ended with `focus_mode=0` and `full_width=0`.

### Native keyboard shortcut proof

The first review found that Command-B and Command-I did not reach their SwiftUI
buttons while the AppKit text view owned the keyboard. The text view now handles
only those two command-key equivalents and sends its current UTF-16 selection
through the same `writing.format` path as the toolbar. Other modifiers and
commands continue down the native responder chain.

On VM1, a mouse-selected range in the first paragraph produced these exact
on-disk results before the fixture was restored:

- Command-B: `se**ssenta e** seis`
- Command-I: `se_ssenta e_ seis`

Each gesture emitted exactly one successful `writing.format` engine operation.
The fixture was restored byte-for-byte after both checks. A pure mapping test
also keeps the native shortcut set closed to Command-B and Command-I.

### Pane-drag review control

An A/B run showed that a real pane header starts the drag and reaches the same
commit path on main `b3cad57` and E1 `6a12d0a`. The provisional new-pane chooser
header has no `paneHeaderDrag`, so it was not a valid control for the original
VNC observation. Dropping a real pane over that provisional surface fails with
`Couldn't move the pane` on both builds; the pre-existing follow-up is #382 and
does not block E1.

## Pane halo proof

The raised surface now wraps the fixed viewport rather than the exact-size
content inside its scroll view. The latter clipped every shadow pixel. The
stationary 1024 screenshot, `E1-halo-1024.png`, contains all four sides:

| Side | canvas | halo sample | face | Result |
|---|---:|---:|---:|---|
| top | `(224,229,236)` | light `(242,244,247)` | `(232,237,244)` | complete |
| left | `(224,229,236)` | light `(239,241,245)` | `(232,237,244)` | complete |
| right | `(224,229,236)` | dark `(186,197,212)` | `(232,237,244)` | complete |
| bottom | `(224,229,236)` | dark `(186,197,212)` | `(232,237,244)` | complete |

## Overlay-scrollbar proof

The AppKit text, screenplay and presentation viewports set
`scrollerStyle = .overlay` and `autohidesScrollers = true`. A SwiftUI bridge
applies the same contract to each writing surface and to the branch scroll view
used when a pane reaches its measured floor. The result is independent of the
machine-wide scrollbar preference.

| Surface | At rest | During scroll | After movement |
|---|---|---|---|
| text viewport, 1024 | no bar | thin overlay bar | disappears after 2 s |
| minimum-overflow branch, 375 | no bar | thin overlay bar | disappears after 2 s |

Evidence: `E1-scroll-inner-rest-1024.png`,
`E1-scroll-inner-active-1024.png`, `E1-scroll-outer-rest-375.png`, and
`E1-scroll-outer-active-375.png`. No legacy gutter is reserved in any capture.

## VM state at handoff

VM1 is running the signed `f42536f` app from `/Applications/Soyeht Dev.app`.
Final PIDs are app `6337`, engine `719`, and PTY daemon `728`. The database was
not reset: it contains one workspace and one live pane (six historical rows are
closed). The text fixture is restored to SHA-256
`e2e310c5672a9b6adcfac6cc7df32904dc221a3559bb1ecc3dd037561db3b297`.
