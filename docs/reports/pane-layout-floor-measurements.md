# Pane layout floor measurements

Date: 2026-09-17

Tree: `cf46dabd042045363097dfcadbb9cf921b638a24` (`origin/main`)

Product: rewrite, bundle `com.soyeht.next.dev.mac`

Measurement machine: VM1, macOS 26.4 (25E246), Retina 2×
Installed app binary: 2026-09-17 21:06:05

This report proposes the numeric floors used by the pane layout policy. It does
not claim that the current SwiftUI implementation already honors them. The
current window still refuses widths below 1024 pt, so the sub-1024 results here
are measurements of the content contract, not screenshots of a window that the
current build cannot create.

## What is measured

Three rectangles are kept separate:

1. **Surface** is the pane card itself, including its header and internal
   insets.
2. **Footprint** is the surface plus the `.pane` halo. The current elevation is
   offset 5 + blur 10, therefore 15 pt on every side and 30 pt on each axis.
3. **Window height** is the footprint plus 84 pt of shell: 8 top + 42 top bar +
   16 inter-row gap + 18 bottom. D8 may redistribute those 84 pt, but must not
   silently increase them.

At the minimum width the rail is an overlay and decorative horizontal margins
have yielded. This is not optional: the largest pane footprint below is 374 pt,
leaving 1 pt in a 375 pt viewport. Keeping the current outer 18 + grid 6 on each
side would make the target mathematically impossible.

The report treats a pane as passing only when its fixture meets every stated
content ruler. A one-point-smaller surface is also evaluated and must fail at
least one ruler. That prevents a copied literal from masquerading as a floor.

## Reproduction

Run from the repository root:

```sh
swift scripts/measure_pane_floors.swift
```

The same script was copied to VM1 and run against the fonts inside the installed
rewrite bundle by setting `SOYEHT_FLOOR_FONT_DIR` to
`/Applications/Soyeht Dev.app/Contents/Resources/Fonts`. Host and VM1 produced
the same metrics and floors. The temporary VM script was removed after the run.

The script registers the bundled TTF files with CoreText, reads the real glyph
advance and line metrics through AppKit, applies the same 2× pixel snapping as
SwiftTerm, and calculates each boundary. It performs no screen automation and
does not mutate application state.

## Font facts

| Use | Source in the app | PostScript font | Point size | measured `ch`/cell | measured line |
|---|---|---:|---:|---:|---:|
| terminal renderer and Preferences | `MacTerminal.swift:17-27`; `PreferencesPanel.swift:714` | `JetBrainsMono-Regular` | 13 | 8.000 | 18 |
| editor | `EditorNativeViewport.swift:8-17` | `JetBrainsMono-Regular` | 11.5 | 6.900 | 19 explicit |
| chrome | `MacStyle.swift:109-119` | `Nunito-Regular` | 12/15 | 7.200/9.000 | 17/21 |
| text | `WritingTextPane.swift:408-418` | `Newsreader16pt-Regular` | 20 | 11.037 | 32 explicit |
| screenplay | `WritingScreenplayPane.swift:709-720` | `CourierPrime-Regular` | 16 | 9.594 | 27 explicit |

The renderer and Preferences now agree on JetBrains Mono 13. VM1 visual
measurement confirms an 18 pt line (36 px at Retina 2×). The terminal surface,
footprint and minimum window height below are therefore 301, 331 and 415 pt.
The layout derives these values from the same `MacTerminalFont` used by the
renderer; the test changes the font metric and proves that the floor follows it.

## Fixtures and rulers

### Terminal

Fixture: 12 rows, each containing 40 `W` cells, selected state strip visible.
SwiftTerm measures the widest ASCII glyph and snaps it to the Retina grid, so
`W` exercises the renderer's own sizing rule.

- width: `40 × 8 + 2 × 10 card inset = 340`
- height: `20 card inset + 26 header + 8 gap + 31 state + 12 × 18 = 301`
- 340×301 passes; 339 gives 39 columns and 300 gives 11 full rows.

The 40×12 target came from the accepted proposal. Font changes recalculate the
formula; they do not create a second literal.

### Editor

Fixture: `scripts/e2e_editor.py` repository, file tree available as a drawer,
dirty Rust buffer, visible cursor at column 36, 12 code rows, status bar and one
active agent-review bar. Long lines retain horizontal scrolling. At compact
width the 212 pt file tree is a drawer; it is not deleted.

- width: `36 × 6.9 code + 44 gutter + 20 text inset + 20 card inset = 332.4`
- height: `20 + 27 header + 8 + 26 tabs + (12 × 19 + 20) viewport + 42 review + 24 status = 395`
- 333×395 passes; 332 exposes 35 columns and 394 loses a full code row.

The 36-column ruler is deliberately smaller than the terminal's 40 because the
editor also owns a 44 pt gutter. It keeps the complete 36-column fixture line,
cursor, diagnostics and horizontal-scroll affordance visible.

### Agent

Fixture: assigned task strip, context/yolo controls, six one-line transcript
rows, and the composer containing 24 `0` characters plus the send button. This
uses the current canonical conversation style.

- width: the nested card/body/conversation/composer insets and button consume
  124 pt; `24 × 9 + 124 = 340`
- height: card/header 54 + task 46 + controls/divider 34 + transcript 150 +
  composer 67 + body top 4 = 355
- 340×355 passes; 339 exposes 23 input characters and 354 clips the sixth
  transcript line.

### Web

Fixture: the local page used by `scripts/e2e_web.py`, with address, responsive
viewport, navigation controls and status bar. At compact width secondary
navigation actions move into one overflow menu; back, forward, reload, address
and that menu remain direct.

- content ruler: 320×240 responsive web viewport
- width: `320 viewport + 20 card inset = 340`
- height: card/header 54 + navigation 27 + two 8 gaps + viewport 240 + status
  24 = 361
- 340×361 passes; 339 produces 319 px of page width and 360 produces 239 px of
  page height.

### Text

Fixture: the Markdown forms exercised by `WritingMarkdownProjection`—heading,
paragraph, nested list, quote, inline code and table—with an active review bar.
The normal column is capped at 66 `ch`; the compact ruler is 24 `ch`, six body
lines and 16 pt adaptive page inset per side. The current fixed 92×68 text inset
is evidence of the implementation to replace, not an input to the floor.

- ideal column: `66 × 11.037 = 728.445 pt`
- compact width: `24 × 11.037 + 32 page inset + 20 card inset = 316.889`
- height: 20 card + 42 shared writing header + 42 toolbar + 42 review + three 8
  gaps + 32 page inset + `6 × 32` lines = 394
- 317×394 passes; 316 exposes 23 `ch` and 393 clips the sixth body line.

The full 66 `ch` footprint is 811 pt, so it becomes available at 1024, not 768.
Below that, prose reflows; it is never scaled down.

### Screenplay

Fixture: `scripts/e2e_writing.py` `night-train.fountain`, with scene/action,
character/dialogue, scene rail available as a drawer, toolbar and a populated
SmartType row. The fixed page is 61 Courier Prime columns plus its existing
74 pt inset on both sides.

- base page width: `61 × 9.59375 + 148 = 733.219 pt`
- minimum legible raster ruler: 7.5 pt type, 46.875% of the 16 pt source
- compact page: 344×446, preserving the 8.5:11 page ratio
- surface height: 42 header + 60 toolbar + 34 SmartType + 446 page = 582
- 344×582 passes; 343 renders at 7.485 pt and 581 clips the scaled page.

At VM1's Retina scale, 7.5 pt yields a 6.8 px lowercase x-height and 8.7 px cap
height. At 7 pt these fall to 6.3 and 8.1 px. The 7.5 pt boundary is the only
visual-legibility threshold in the arithmetic and should be accepted explicitly.

### Presentation

Fixture: `scripts/e2e_writing.py` `review.md`, three slides with title, notes and
image. In compact mode the thumbnail rail becomes a numeric strip. Header,
numeric selection, stage controls and footer remain reachable.

- content ruler: complete 320×180 slide, exactly 16:9
- width: `320 stage + 20 internal inset = 340`
- height: 42 header + 32 numeric strip + 180 stage + 28 footer + 20 inset = 302
- 340×302 passes; 339 makes the stage 319 wide and 301 makes it 179 high.

The stage grows uniformly until height becomes the limiting axis; it never
deforms.

## Proposed floors

| Type | surface | footprint with halo | minimum window height | failing neighbor |
|---|---:|---:|---:|---:|
| terminal (actual default) | 340×301 | 370×331 | 415 | 369 wide / 414 high |
| editor | 333×395 | 363×425 | 509 | 362 wide / 508 high |
| agent | 340×355 | 370×385 | 469 | 369 wide / 468 high |
| web | 340×361 | 370×391 | 475 | 369 wide / 474 high |
| text | 317×394 | 347×424 | 508 | 346 wide / 507 high |
| screenplay | 344×582 | 374×612 | 696 | 373 wide / 695 high |
| presentation | 340×302 | 370×332 | 416 | 369 wide / 415 high |

The “failing neighbor” is in window coordinates: width includes the halo and
height includes halo plus shell. Surface-level one-point failures are described
in each fixture section.

## Width matrix

All requested widths pass every footprint; the cells show what remains visible,
not merely `P`. “Drawer” means the secondary region still exists and is
reachable. Presentation sizes use an 812 pt-high window; from 1440 onward height
limits the stage.

| window width | terminal cols | editor code cols | agent input `ch` | web viewport width | text column | screenplay zoom | slide stage |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 375 | 40 | 37, tree drawer | 24 | 325 | 26 compact | 47.1% | 325×183 |
| 390 | 42 | 40, tree drawer | 26 | 340 | 27 compact | 49.1% | 340×191 |
| 430 | 47 | 45, tree drawer | 30 | 380 | 31 compact | 54.6% | 380×214 |
| 768 | 89 | 64, tree visible | 68 | 718 | 62 compact | 100% | 718×404 |
| 1024 | 121 | 101, tree visible | 96 | 974 | 66 full | 100% | 974×548 |
| 1440 | 173 | 161, tree visible | 142 | 1390 | 66 full | 100% | 1024×576 |
| 3440 | 423 | 451, tree visible | 365 | 3390 | 66 full | 100% | 1024×576 |

At 3440 a single pane still grows with the window, per D6. Reading surfaces do
not: text remains 66 `ch`, screenplay remains a full-size page, and the 16:9
stage is height-limited and centered. The remainder is margin or secondary
surface, not stretched text.

## Height matrix and complete cross-product

Heights tested: 812, 768, 736, 672, 640, 568, 480, 430, 390 and 375 pt.
`P` means the complete fixture fits; `F` means the pane must make its branch
scroll rather than squeeze below its floor.

| window height | terminal | editor | agent | web | text | screenplay | presentation |
|---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| 812 | P | P | P | P | P | P | P |
| 768 | P | P | P | P | P | P | P |
| 736 | P | P | P | P | P | P | P |
| 672 | P | P | P | P | P | F | P |
| 640 | P | P | P | P | P | F | P |
| 568 | P | P | P | P | P | F | P |
| 480 | P | F | P | P | F | F | P |
| 430 | P | F | F | F | F | F | P |
| 390 | F | F | F | F | F | F | F |
| 375 | F | F | F | F | F | F | F |

The width and height tests are separable rectangles. Because every requested
width (375/390/430/768/1024/1440/3440) is at least the largest footprint width
(374), the table above is the height result for **each of the seven requested
widths**. This is the full type × width × height Cartesian matrix without
repeating the same ten symbols seven times.

Examples: 375×812 passes every type; 375×672 fails only screenplay; 3440×480
passes terminal/agent/web/presentation and scrolls editor/text/screenplay;
3440×390 scrolls every type.

## Consequences for the SwiftUI stage

- The policy stores content rulers/formulas, not the rounded values in this
  report. Changing a font metric must change the derived floor in tests.
- `.pane` halo clearance is 15 pt per side in one shared calculation.
- At 375 width, horizontal shell decoration has no budget beyond the halo. The
  rail must overlay and current outer/grid padding must yield.
- At compact editor widths the file tree is a drawer; at compact web/text widths
  secondary toolbar actions are overflow controls. “Compact” never means
  absent.
- A branch scrolls as soon as the window allocation would cross a failing
  neighbor. No child receives less than its footprint.
- The screenplay floor is the tallest and sets the single-pane vertical
  boundary: 696 pt. It still fits the 375×812 target with 116 pt spare.
- The hard-coded `.frame(minWidth: 1024, minHeight: 640)`, editor 620 pt floor,
  writing 860 pt preference and fixed writing insets are implementation inputs
  to remove, not accepted measurements.

No SwiftUI was changed for this report.
