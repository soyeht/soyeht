# iPhone writing layout policy

Date: 2026-09-20

Base tree: `8a4bc31` (`main` after workspace-tab block B)

Scope: block D1, pure shared-client policy and device-local state only. This
change does not add an iPhone view, persist to `UserDefaults`, change the wire,
or change the engine.

## Method

The accepted ruler is the iPhone 13 mini portrait viewport, `375×812` pt. The
test fixture uses its `50` pt top and `34` pt bottom safe areas. It evaluates
widths `375`, `390`, and `430`; heights `812`, `667`, `568`, `430`, and `375`;
and each cell both unobscured and with a `291` pt software keyboard.

Run the focused matrix from the repository root:

```sh
swift test --package-path apps/shared --filter PhoneWritingLayoutPolicy
```

The policy receives the complete viewport and safe-area/keyboard obstructions.
It measures once and returns rectangles. The later SwiftUI layer must render
those rectangles; it does not make a second pressure decision.

## Named metrics and formulas

The phone shell uses these named metrics:

| metric | value | reason |
|---|---:|---|
| minimum exercised viewport width | 375 pt | Caio's iPhone mini ruler |
| minimum hit target / title row | 44 pt | touch and accessibility floor |
| companion tab row | 44 pt | the same hit-target floor |
| horizontal inset | 15 pt per side | leaves 345 pt at the 375 pt ruler, above every measured writing floor |
| vertical inset | 12 pt per side | separates headers from safe-area edges |
| row spacing | 12 pt | visible boundary between independently collapsible leaves |
| screenplay pinch multiplier | 1...4× | whole-page fit is reset; pinch is bounded local state |

The usable rectangle is:

```text
width  = viewport − safe leading/trailing − 2 × 15
height = viewport − safe top/bottom − 2 × 12 − keyboard obstruction
```

An unpaired collapsed leaf is exactly one `44` pt header. A paired collapsed
leaf is a `44` pt title row plus the always-visible `44` pt Document/Agent tab
row. When expanded, a leaf receives the greater of its measured content floor
and the remainder of the visible height. Extra rows create vertical scrolling.
The content extent includes the bottom safe area, shell inset, and keyboard
obstruction so the final row can actually scroll above an overlaid keyboard;
the policy never creates horizontal scrolling.

The content floors reuse the accepted C2 measurements and remove the `42` pt
Mac writing header because the phone shell owns its header:

| kind | accepted surface | phone content floor | complete unpaired row |
|---|---:|---:|---:|
| text | 317×394 | 317×352 | 396 pt tall |
| screenplay | 344×582 | 344×540 | 584 pt tall |
| presentation | 340×302 | 340×260 | 304 pt tall |
| explicit screenplay/agent pair | max with agent 340×355 | 344×540 | 628 pt tall, including both header rows |

## Width matrix

All three accepted widths retain a full-width leaf with no horizontal overflow.
The screenplay is one fixed `8.5:11`, 61-column Courier page. Its reset scale
is derived from the usable width and never falls below `7.5/16 = 46.875%`.

| viewport | usable width | screenplay whole-page fit |
|---:|---:|---:|
| 375 | 345 | 47.053% |
| 390 | 360 | 49.099% |
| 430 | 400 | 54.554% |

`374` pt is tested as one point below the accepted device ruler and is named as
unsupported rather than silently accepted. It still does not mutate the shared
pane tree.

## Height and keyboard matrix

The table records the usable height after the safe area and shell insets.
`P` means one expanded leaf fits without outer scrolling; `S` means the outer
vertical list scrolls while the leaf keeps its floor.

| viewport height | usable | text | screenplay | presentation | screenplay+agent |
|---:|---:|:---:|:---:|:---:|:---:|
| 812 | 704 | P | P | P | P |
| 667 | 559 | P | S | P | S |
| 568 | 460 | P | S | P | S |
| 430 | 322 | S | S | P | S |
| 375 | 267 | S | S | S | S |

With the `291` pt keyboard, the usable sequence becomes `413`, `268`, `169`,
`31`, and `0` pt. At `812`, text and presentation still fit; screenplay and a
pair scroll at their floors. At smaller heights every kind that does not fit
scrolls instead of shrinking. The focused test separately proves this for one
screenplay leaf so the full matrix cannot hide it behind multiple rows.

## Canonical projection and local state

The projection walks `PaneLayoutTree` geometry order. It emits text,
screenplay, and presentation once each. An agent joins a leaf only when a valid
`PaneLayoutCompanion` names exactly those two direct leaves; adjacency never
infers a pair. Terminal, editor, web, unknown-writing, and unpaired agents are
outside D1.

State keys use `paneId` plus `fileId`. With no stored state, the selected
writing pane opens; selecting the explicit companion opens the same leaf on its
Agent tab; otherwise the first canonical writing leaf opens. Existing all-
collapsed state remains all collapsed. Reconciliation prunes dead identities,
does not lend state to a replacement file, removes an Agent selection when the
pair disappears, and clamps screenplay zoom. The versioned JSON payload is
device-local and never enters `workspace.layout`.

## Automated evidence

The 13 focused tests cover:

- canonical projection, explicit pairing, and rejection of inference;
- selected/first-canonical initial state;
- stored collapse, dead-ID pruning, and changed file identity;
- local-state round-trip, malformed input, and unknown versions;
- bounded pure actions for collapse, tabs, and screenplay zoom;
- missing/inconsistent document states without inventing panes;
- complete names and `44` pt title/tab targets;
- validated font and document `fullWidth` carried into the view plan;
- the width, height, safe-area, and keyboard matrix;
- the one-point device-width boundary;
- screenplay keyboard pressure scrolling instead of shrinking.

D2 will add the persistence adapter and the list/header/text views. D3 adds the
fixed-page pinch surface. D4 adds presentation and the visible companion Agent
tab. Those stages must consume this policy rather than reopen its decisions in
SwiftUI.
