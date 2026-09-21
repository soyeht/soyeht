# Writing chrome E2 validation

Date: 2026-09-20

Build under test: `f34bf13` (`codex/writing-chrome-e2`)

Product: rewrite, bundle `com.soyeht.next.dev.mac`

Machine: VM1, macOS 26.4, Retina 2×. The signed app was installed at
`/Applications/Soyeht Dev.app`; no UI work was performed on the host.

This report covers the screenplay half of P5/P2 writing chrome. It removes the
fixed toolbar and fixed SmartType row, applies the common 40 pt writing header,
keeps Fountain on one aspect-fit page, and moves element choice plus SmartType
to the active empty line. It does not change presentation chrome.

## Accepted measurements

The view consumes the named values in `WritingChromeMetrics` and
`PaneLayoutContentMetrics`; these values are also asserted by pure tests.

| Element | Measurement | Rendering |
|---|---:|---|
| common header | 40 pt high | `face` |
| header horizontal inset | 18 pt | both edges |
| file name | Nunito 14/600 | `ink-secondary` |
| scene count | Nunito 14/400 | `ink-muted` |
| options glyph | 16 pt in 32 pt target | `ink-secondary` |
| element menu | 148×30 pt | Nunito 12 bold, 12 pt horizontal inset |
| inline control gap | 6 pt | named metric |
| SmartType label and choices | Courier Prime 11 bold | raised 30 pt controls |
| page | 61 Courier Prime columns | fixed 8.5:11; never reflows |

Removing the 60 pt toolbar and 34 pt SmartType row changes the derived
screenplay surface floor from 344×582 to **344×486**. With the shared 15 pt
halo on every side, the footprint is **374×516**. The compact raster remains
46.875% (7.5 pt from a 16 pt source); pressure below the floor scrolls rather
than shrinking the page further.

## Action inventory

No screenplay door was removed.

| Door | Actions |
|---|---|
| empty active line | element menu and bounded SmartType suggestions |
| element menu | Scene Heading, Action, Character, Parenthetical, Dialogue, Transition, Shot |
| `Screenplay options` | Idea Board, Scenes, Focus, split columns, split rows, close pane |

The element menu appears only for a collapsed insertion selection on an empty
line. SmartType is derived from `ScreenplayInteraction.suggestions`, limited to
five known facts, and shares that inline row. After either door inserts text,
the line is no longer empty and the controls disappear instead of covering the
new text. The native borderless menu indicator is hidden, leaving the single
explicit chevron in the 148 pt control.

The header and inline row expose named accessibility containers and controls.
The common dimming policy remains in force: sustained editing dims chrome to
0.35, while pointer movement, menus, popovers, hover, and pane drag restore it.

## Pure and automated proof

The focused `ScreenplayInteractionTests` passed 6/6. They cover bounded
SmartType facts and the element kind inferred at the active line.

The writing and pane policy suites assert:

- the 40 pt header is the only fixed screenplay chrome in the layout;
- the screenplay has no `toolbarRect`;
- 344×486 passes and its one-point neighbors overflow;
- the page remains 8.5:11 and never rasterizes below 7.5 pt;
- the inline menu is restricted to an empty insertion line;
- the accepted 30/12/6/11 pt inline metrics have one named source.

The full shared suite passed **232/232**, the focused pane-layout suite passed
**67/67**, and the signed `scripts/check_mac.sh --sign` run (development team
`ABCDE12345`) exited 0 with `BUILD SUCCEEDED`.

## VM1 visual and interaction proof

Evidence is preserved under `~/evidence-e2` on VM1.

| Viewport | Result | Evidence |
|---:|---|---|
| 1024×900 | one-tone 40 pt header; fixed page; complete halo; no fixed toolbar | `E2-clean-1024.png`, `E2-final-clean-1024.png` |
| 1024×900, empty line | 148×30 element menu, SmartType label and `MARIA`; one chevron | `E2-inline-1024.png` |
| 700×600 | page keeps its ratio and scrolls at its derived floor | `E2-700x600.png` |
| 375×672 | inline controls remain reachable; options menu is not clipped | `E2-inline-375x672.png`, `E2-options-375x672.png` |

The seven element choices were opened from the inline menu. Selecting `MARIA`
inserted `@MARIA` through the real edit path and immediately dismissed the
inline row (`E2-smarttype-applied-1024.png`). The fixture was then restored.

The six header actions were exercised: Idea Board opened; Scenes showed all
eight scenes; Focus changed the stored flag 0→1→0; split columns and split rows
created their provisional leaves and Escape canceled them; closing the clean
screenplay closed without terminal-specific confirmation. The file remained
byte-identical after the run:
`81de8d425a7d02eea33c74b574e3d15096c3b9496d339edec4b4bab82791c70d`.

## Overlay-scrollbar proof

The first narrow split exposed a nested-scroll defect: the AppKit bridge
configured only the nearest scroll view and returned, so the outer branch could
still inherit the machine's permanent legacy scroller. The bridge now walks
above the nearest owner and applies `.overlay` plus `autohidesScrollers` to
every enclosing `NSScrollView`.

At 375 pt, horizontal wheel input moved the minimum-floor branch and displayed
a thin overlay thumb (`E2-scroll-outer-active-375.png`). Five seconds after
movement, the thumb was gone and no gutter remained
(`E2-scroll-outer-rest-375.png`). The same policy remains active on the inner
screenplay viewport and the other two writing kinds.

## Clean relaunch and handoff state

Before handoff the rewrite app and both rewrite services were stopped. Logs
were moved to
`~/soyeht-logs-before-writing-e2-20260920-215027`; all other accumulated
rewrite state was moved to
`~/soyeht-state-before-writing-e2-20260920-215027`. The installed binaries and
LaunchAgents remained. `com.soyeht.next.dev.ios.identity` was absent from the
keychain. `harness-logs` and `experiments` were absent.

The clean database started at `0|0|0`. The preserved screenplay fixture was
then opened for review, so the handoff database is `1|1|1`. Final PIDs are app
`11647`, engine `11637`, and PTY daemon `11635`; the window is 1024×900. The app
binary is dated 21:43:01 and the engine/PTY binaries 16:09:50.
