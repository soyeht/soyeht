# Writing chrome E3 validation

Date: 2026-09-20

Build under test: `eedb1db` (`codex/writing-chrome-e3`)

Product: rewrite, bundle `com.soyeht.next.dev.mac`

Machine: VM1, macOS 26.4, Retina 2×. The signed app was installed at
`/Applications/Soyeht Dev.app`; no UI work was performed on the host.

This report closes the Mac P5/P2 writing-chrome pass. Presentation now uses the
same one-tone 40 pt header as text and screenplay, with its title, count, and
actions in the accepted hierarchy. It also records the final 1024/700/375
matrix across all three writing kinds.

## Accepted measurements

The view and floor formula consume the same named values from
`WritingChromeMetrics` and `PaneLayoutContentMetrics`.

| Element | Measurement | Rendering |
|---|---:|---|
| common header | 40 pt high | `face` |
| header horizontal inset | 18 pt | both edges |
| file name | Nunito 14/600 | `ink-secondary` |
| slide count | Nunito 14/400 | `ink-muted` |
| options glyph | 16 pt in 32 pt target | `ink-secondary` |
| options menu | 232 pt wide; 28 pt rows | `face`, non-focusable rows |
| compact slide rail | 32 pt | numbered controls |
| minimum stage | 320×180 pt | exact 16:9 |
| maximum stage | 1024×576 pt | exact 16:9 |
| thumbnail rail | 236 pt | `face` |
| footer | 28 pt | `face` |
| chrome while typing | 0.35 opacity | 0.16 s fade |
| chrome on pointer/menu/sheet | 1.0 opacity | 0.10 s reveal |

Presentation no longer pays the retired 42 pt header. Its derived content floor
is now **340×300 pt**: 320×180 stage, 40 pt header, 32 pt compact rail, 28 pt
footer, and 10 pt pane inset on each edge. The shared 15 pt pane halo makes the
complete footprint **370×330 pt**. One point below either accepted floor is
explicit overflow rather than silent compression.

The E2 inline screenplay row also gains the named 3 pt anchor clearance. Its
30 pt controls now clear the preceding 27 pt line pitch without changing the
page, type, or compact-scale policy.

## Action inventory

No presentation door was removed.

| Door | Actions |
|---|---|
| `Presentation options` | Present, split columns, split rows, close pane |
| slide rail | select one of the parsed slides |
| stage | edit the presentation Markdown |
| presentation sheet | previous, next, done |

The fixed Present button is gone. Present and pane mutations live under the
single header menu, which has a named accessibility container and named rows.
The common header exposes the complete file name to accessibility even when the
visible title truncates. A clean presentation closes immediately; a dirty one
uses writing-specific copy (`Close deck.md?`) rather than the terminal warning.

## Pure and automated proof

The focused chrome and writing-layout suites passed **40/40**. They cover the
accepted P2 measurements, all retained menu doors, dim/reveal transitions,
writing-specific close policy, the 16:9 stage boundaries, numeric versus
thumbnail navigation, and the complete requested viewport matrix.

The pane-layout gate passed **67/67**, including derived floors, one-point
overflow, split insertion, resize floors, custom-layout persistence, companion
presentation, and stable pane identity.

The full shared suite passed **234/234**. A direct signed
`scripts/check_mac.sh --sign` run (development team `<TEAM_ID>`) repeated the
67-test pane-layout gate, printed the integral shared suite as green, and
finished with `BUILD SUCCEEDED`.

## VM1 visual matrix

Evidence is preserved under `~/evidence-e3` on VM1.

| Kind | 1024×900 | 700×600 | 375×672 |
|---|---|---|---|
| text | 66ch centered column, complete halo, one tone | column clamps without fixed toolbar | compact column scrolls; header remains complete |
| screenplay | fixed 8.5:11 page, complete halo, one tone | page scales as one object | whole page at compact scale; no horizontal reflow |
| presentation | thumbnail rail and 16:9 stage | thumbnail rail and 16:9 stage | numbered rail and 16:9 stage |

Evidence files are `e3-text-{1024,700,375}.png`,
`e3-screenplay-{1024,700,375}.png`, and
`e3-presentation-{1024,700,375}.png`. In every cell the card halo closes on all
four sides; the header, rail, stage/page, and footer use `face`; no fixed writing
toolbar returns; and no permanent scrollbar gutter is present.

The presentation's nested slide rail initially inherited the VM's permanent
legacy scrollbar preference. A marker inside that rail now applies the same
`.overlay` plus autohide policy as every other writing scroll owner. At rest it
draws no bar (`e3-presentation-overlay-1024.png`); real wheel input moved the
rail while retaining the thin overlay policy
(`e3-presentation-overlay-active-1024.png`). The horizontal number strip has
the same owner-local policy and keeps its intentionally hidden indicator.

## VM1 interaction proof

The header menu rendered four rows with no accent focus ring
(`e3-presentation-menu-1024.png`). Present opened the 16:9 slide sheet with the
parsed eight-slide count (`e3-presentation-present-1024.png`). Split columns and
split rows created provisional leaves beside and below the presentation while
keeping the original pane mounted; cancel removed each provisional leaf without
creating a document (`e3-presentation-split-columns.png`,
`e3-presentation-split-rows.png`).

Typing through the real AppKit stage dimmed the shared header to 0.35
(`e3-presentation-header-dimmed.png`). A real pointer-moved event restored it
to full opacity (`e3-presentation-header-restored.png`). The fixture was then
restored and verified at SHA-256
`8fa8c719a24e779eebf05e1c595968a84d871c7b7a86541ba066ed6a2bc0aa86`.

Closing the clean presentation removed it immediately with no terminal copy
(`e3-presentation-close-clean.png`). Reopening the same document from a second
client repopulated the already-running app without a kickstart, retaining the
cross-client fix from #365 (`e3-presentation-recreated.png`).

The 375 pt proof also exercised the workspace `+2` list: both hidden workspaces
were named in full, selecting one made it the sticky active tab, and no
workspace vanished without an indicator (`e3-workspace-hidden-menu-375.png`).

## Clean relaunch and handoff state

After the signed gate, VM1 was stopped in engine/ptyd/app order. The previous
logs were moved intact to
`~/soyeht-logs-before-writing-e3-final-20260920-232204/` (`engine.log` and
`ptyd.log`). The previous Soyeht-owned state was moved to
`~/soyeht-state-before-writing-e3-final-20260920-232204/`: the application
preferences plist, `engine.lock`, `ptyd-registry.json`, `ptyd.lock`,
`ptyd.sock`, `sessions/`, and `soyeht.db`. `harness-logs/` and `experiments/`
were absent. The installed binaries and LaunchAgents were retained. The
`com.soyeht.next.dev.ios.identity` Keychain service was absent (status 44).

The signed rewrite app was installed at `/Applications/Soyeht Dev.app` and the
two rewrite agents were bootstrapped before opening bundle
`com.soyeht.next.dev.mac`. The installed app binary time is
**2026-09-20 22:55:10 -03**. The new processes are ptyd **18237**, engine
**18239**, and app **18256**, started at 23:23:17, 23:23:17, and 23:23:20 -03.

Before adding review data, the new database was verified at **0 workspaces, 0
panes, 0 writing documents**. A second RPC client then created only workspace
`E3 Presentation` and opened the preserved external fixture
`~/c3-texto/Writing/Presentations/deck.md`. It appeared in the already-running
app without a kickstart. The handoff database is therefore deliberately
**1|1|1**, the window is **1024×900**, and the fixture remains at SHA-256
`8fa8c719a24e779eebf05e1c595968a84d871c7b7a86541ba066ed6a2bc0aa86`.
The final screen is preserved as `e3-clean-handoff.png` with the other review
evidence.
