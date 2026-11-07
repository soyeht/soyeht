# Writing layout policy measurements

Date: 2026-09-19

Base tree: `da5002384babd4e968248dd914d680726f7d4a96` (`origin/main`)

Policy commit: `bdd8c19`

Product: rewrite, bundle `com.soyeht.next.dev.mac`

This report records the inputs and boundaries for the pure C2 writing-layout
policy. C2 does not wire these plans into SwiftUI, change the persisted pane
tree, or infer a writing/agent pair. Those integrations remain in C3-C6.

## Method

Run from the repository root:

```sh
swift scripts/measure_pane_floors.swift
swift test --package-path apps/shared --filter WritingLayoutPolicy
```

The measurement script registers the fonts shipped in the Mac app and asks
CoreText for the real advance of `0`. The policy test then evaluates the seven
requested widths (375, 390, 430, 768, 1024, 1440 and 3440), the ten requested
heights (812, 768, 736, 672, 640, 568, 480, 430, 390 and 375), and all three
text fonts. One-point neighbors exercise every state boundary.

`WritingLayoutPolicy` consumes a **pane-surface viewport**. A Mac window also
owns 15 pt of halo on each pane edge and 84 pt of vertical shell. Therefore:

- surface width = single-pane window width - 30;
- surface height = single-pane window height - 114;
- a branch scrolls when the supplied surface is below its measured floor.

The tests use surface coordinates directly. The final table translates the
same floors back to window coordinates for comparison with the earlier floor
report.

## Font facts

| document font | PostScript font | size | measured `ch` | policy line |
|---|---|---:|---:|---:|
| reading serif | `Newsreader16pt-Regular` | 20 | 11.037048 | 32 |
| classic serif | `TimesNewRomanPSMT` | 20 | 10.000000 | 32 |
| sans | `Nunito-Regular` | 20 | 12.000000 | 32 |
| screenplay | `CourierPrime-Regular` | 16 | 9.593750 | 27 |

The script also reports each font's raw CoreText line box (20, 23, 28 and 18
pt respectively). Text uses the existing explicit 32 pt paragraph ruler and
screenplay uses the existing explicit 27 pt ruler; raw boxes do not silently
replace those view metrics.

## Text

Reading mode caps the column at `66 × active ch`, then centers it. It reflows
down to a 24-character compact ruler; it never scales. Full-width mode removes
only the 66-character cap. Both modes retain the same header, toolbar, review
bar, page insets and active font.

The target compact surface formula is:

```text
width  = ceil(24 × active ch + 2 × 16 page inset + 2 × 10 pane inset)
height = 2 × 10 + 42 header + 42 toolbar + 42 review
       + 3 × 8 gaps + 2 × 16 page inset + 6 × 32 lines
```

| font | compact surface | 66ch column | surface needed for 66ch | footprint needed |
|---|---:|---:|---:|---:|
| Newsreader | 317×394 | 728.445 | 781 | 811 |
| Times New Roman | 292×394 | 660 | 712 | 742 |
| Nunito | 340×394 | 792 | 844 | 874 |

The supplied global pane floor remains an additional lower bound. With the
current 317 pt global text floor, Times New Roman may receive 317 rather than
292; wider active fonts still raise the effective floor (Nunito raises it to
340). This keeps the policy safe while the tree still stores floors by kind.

At the requested surface widths, the visible reading columns are:

| surface width | Newsreader | Times | Nunito |
|---:|---:|---:|---:|
| 375 | 29.3ch | 32.3ch | 26.9ch |
| 390 | 30.6ch | 33.8ch | 28.2ch |
| 430 | 34.2ch | 37.8ch | 31.5ch |
| 768 | 64.9ch | 66ch | 59.7ch |
| 1024 / 1440 / 3440 | 66ch | 66ch | 66ch |

## Screenplay

The page is fixed at 61 Courier Prime columns plus its 74 pt internal inset on
each side. It aspect-fits as one page and never reflows or deforms.

```text
base page width = 61 × 9.59375 + 148 = 733.21875
minimum scale   = 7.5 / 16 = 46.875%
compact page    = 344 × 446 (8.5:11)
compact surface = 344 × (42 header + 60 toolbar + 34 SmartType + 446) = 344×582
```

The scene rail is present when the surface is at least
`330 rail + 1 divider + 733.21875 page = 1064.21875` pt wide. At one point less
it becomes a drawer. The accepted minimum scale is a hard floor: smaller
allocations scroll rather than rasterizing below 7.5 pt.

At a surface height of 812, the two-axis result is:

| surface width | navigation | page scale |
|---:|---|---:|
| 375 | drawer | 51.1% |
| 390 | drawer | 53.2% |
| 430 | drawer | 58.6% |
| 768 | drawer | 71.2% (height-limited) |
| 1024 | drawer | 71.2% (height-limited) |
| 1440 / 3440 | rail | 71.2% (height-limited) |

## Presentation

The stage aspect-fits at exactly 16:9, from the accepted 320×180 minimum to a
named 1024×576 maximum. It never deforms. The thumbnail rail becomes a numeric
strip when the surface is one point below this boundary:

```text
236 thumbnail rail + 1 divider + 2 × 10 stage inset + 320 stage = 577
```

At a surface height of 812:

| surface width | navigation | stage |
|---:|---|---:|
| 375 | numbers | 355×199.7 |
| 390 | numbers | 370×208.1 |
| 430 | numbers | 410×230.6 |
| 768 | thumbnails | 511×287.4 |
| 1024 | thumbnails | 767×431.4 |
| 1440 / 3440 | thumbnails | 1024×576 |

The compact surface is 340×302. Every requested surface height is at least 375,
so the complete stage fixture fits throughout the direct policy matrix.

## Explicit writing/agent companion

Only a persisted `PaneLayoutCompanion` activates this policy. The two direct
leaves remain in canonical tree order; the policy never picks the first agent
and never rewrites the tree.

| writing kind | writing floor | side-by-side at width | below at height | tabs content floor |
|---|---:|---:|---:|---:|
| text (current global floor) | 317×394 | 675 | 767 | 340×430 |
| screenplay | 344×582 | 702 | 955 | 344×618 |
| presentation | 340×302 | 698 | 675 | 340×391 |

Each side threshold is `writing width + 18 gap + 340 agent`; each below
threshold is `writing height + 18 gap + 355 agent`. At the exact boundary the
larger presentation is used. One point below both boundaries produces a
36 pt tab strip whose two tabs are always present.

## Complete requested matrix

All seven requested **surface widths** exceed every compact writing floor.
The height result is therefore identical at each width. `P` means the complete
fixture fits; `S` means the branch scrolls at its measured floor.

| kind | 812 | 768 | 736 | 672 | 640 | 568 | 480 | 430 | 390 | 375 |
|---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| text | P | P | P | P | P | P | P | P | S | S |
| screenplay | P | P | P | P | P | S | S | S | S | S |
| presentation | P | P | P | P | P | P | P | P | P | P |

For a single-pane **Mac window**, halo and shell produce these boundaries:

| kind | surface | footprint | minimum window height | height sequence |
|---|---:|---:|---:|---|
| text | 317×394 | 347×424 | 508 | `PPPPPPSSSS` |
| screenplay | 344×582 | 374×612 | 696 | `PPPSSSSSSS` |
| presentation | 340×302 | 370×332 | 416 | `PPPPPPPPSS` |

The width sequence is `P` for all seven requested Mac widths because the
largest footprint is 374 pt and the smallest requested window is 375 pt.

## Test boundaries and exclusions

The pure tests prove:

- 66ch follows three active font advances and full width removes only the cap;
- 24ch and six lines are the text floor, with scroll one point below;
- screenplay remains 8.5:11 and never falls below 46.875%;
- screenplay rail/drawer and presentation thumbnails/numbers switch at exact
  measured boundaries and at their one-point neighbors;
- every slide remains 16:9 and stays within 320×180...1024×576;
- explicit companions choose side, below and tabs without changing canonical
  order; invalid pairs and unsupported kinds are rejected;
- all 350 requested kind/font/viewport cells preserve their invariants.

C2 deliberately contains no SwiftUI, no nested `GeometryReader`, no protocol or
engine change, no new persistence, no automatic pair inference, and no keyboard
shortcut. Views will consume these plans in the later C3-C6 PRs.
