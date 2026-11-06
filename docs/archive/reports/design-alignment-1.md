# Design alignment pass 1

Date: 2026-09-12

Implementation base: `84b97a6`

Implementation head before this report: `1b43de0`

## Result

The Mac screens shipped in slices 1–3 now use one neomorphic theme and match the approved `.pen` frames on palette, spacing, type, radii, elevation, component shape, and placement. The comparison used the running Dev app at a 1440 × 900 logical window size and direct renders of the approved frames.

All presentation changes stayed inside `apps/mac`, its visual checks, the alignment ADRs, and this report. `contracts/protocol.json`, the engine, the store, `README.md`, and `design/soyeht-desktop.pen` did not change during this pass.

## Canonical token map

`apps/mac/Sources/MacStyle.swift` is the single source used by every aligned Mac screen. The `.pen` variable names and their Swift tokens are:

| `.pen` token | Value | Swift token |
| --- | --- | --- |
| `canvas` | `#E0E5EC` | `MacTheme.canvas` |
| `face`, `terminal-bg` | `#E8EDF4` | `MacTheme.face` |
| `well` | `#D8DEE8` | `MacTheme.well` |
| `hover` | `#DCE2EA` | `MacTheme.hover` |
| `border` | `#D8DEE8` | `MacTheme.border` |
| `ink`, `terminal-ink` | `#3E4A66` | `MacTheme.ink` |
| `secondary` | `#6E7A96` | `MacTheme.secondary` |
| `muted` | `#8E9AB3` | `MacTheme.muted` |
| `accent` | `#5B7CFA` | `MacTheme.accent` |
| `on-accent` | `#FFFFFF` | `MacTheme.onAccent` |
| `shadow-dark` | `#A6B4C8` | `MacTheme.shadowDark` |
| `shadow-light` | `#FFFFFF` | `MacTheme.shadowLight` |
| `selection` | `#C9D6EE` | `MacTheme.selection` |
| `success` | `#2E9A73` | `MacTheme.success` |
| `warning` | `#B98A2E` | `MacTheme.warning` |
| `danger`, `urgent` | `#D9556A` | `MacTheme.danger`, `MacTheme.urgent` |
| agent blue | `#D8E0FE` | `MacTheme.agentBlue` |
| agent green | `#CDE7DD` | `MacTheme.agentGreen` |
| agent rose | `#F6D6DB` | `MacTheme.agentRose` |
| agent sand | `#EEE3CD` | `MacTheme.agentSand` |

| `.pen` scale | Values | Swift token |
| --- | --- | --- |
| radii | `10`, `12`, `20`, `999` | `MacTheme.Radius` |
| spacing | `2`, `3`, `4`, `5`, `6`, `7`, `8`, `10`, `12`, `14`, `16`, `18` | `MacTheme.Space` |
| type sizes | `10`, `11`, `12`, `13`, `15`, `18`, `20`, `28` | `MacTheme.TypeScale` |
| chrome family | Nunito | `MacTheme.chrome` |
| code family | JetBrains Mono | `MacTheme.mono` |
| chip elevation | offset `3`, blur `5` | `MacTheme.Elevation.chip` |
| toggle elevation | offset `2`, blur `4` | `MacTheme.Elevation.toggle` |
| task-card elevation | offset `3`, blur `6` | `MacTheme.Elevation.taskCard` |
| action elevation | offset `4`, blur `8` | `MacTheme.Elevation.action` |
| card elevation | offset `4`, blur `9` | `MacTheme.Elevation.card` |
| pane elevation | offset `5`, blur `10` | `MacTheme.Elevation.pane` |
| pill elevation | offset `6`, blur `10` | `MacTheme.Elevation.pill` |
| panel elevation | offset `9`, blur `18` | `MacTheme.Elevation.panel` |

Raised surfaces use paired light top-left and dark bottom-right shadows. Wells use the same pair as inner shadows. Accent controls use the approved blue face with a restrained blue glow and the light top-left shadow.

## Screen comparisons

### Workspace and panes · Tela 1

| Approved frame `l5YU91` | Running Mac app |
| --- | --- |
| ![Approved workspace frame](design-alignment-1/reference/l5YU91.png) | ![Running workspace](design-alignment-1/app/workspace.png) |

**Pass.** The tab strip, workspace ground, pane grid, headers, status colors, controls, radii, spacing, and dual shadows use the canonical tokens. The selected terminal is live. Inactive terminal panes use published preview facts under ADR 0036.

### New pane · Tela 2

| Approved frame `nOYUL` | Running Mac app |
| --- | --- |
| ![Approved new-pane frame](design-alignment-1/reference/nOYUL.png) | ![Running new-pane selector](design-alignment-1/app/new-pane.png) |

**Pass.** The full-column entry, centered 360 × 344 selector, harness rows, model and effort wells, type hierarchy, selection treatment, and footer action follow the approved component.

### Agent pane · Tela 5

| Approved frame `q4tKe` | Running Mac app |
| --- | --- |
| ![Approved agent-pane frame](design-alignment-1/reference/q4tKe.png) | ![Running agent pane](design-alignment-1/app/agent-pane.png) |

**Pass.** The agent header, state treatment, conversation surface, task context, composer well, action pills, chrome type, and code type use the shared theme. Raw system fonts are rejected by the Mac text-font gate.

### Task board · Tela 3

| Approved frame `N0IEF` | Running Mac app |
| --- | --- |
| ![Approved task-board frame](design-alignment-1/reference/N0IEF.png) | ![Running task board](design-alignment-1/app/task-board.png) |

**Pass.** The board/list control, filters, dispatch and create actions, columns, cards, state dots, priorities, badges, task identifiers, and empty column spacing match the approved structure. The running image shows engine-published task data.

### Task list · Tela 7

| Approved frame `hS4wB` | Running Mac app |
| --- | --- |
| ![Approved task-list frame](design-alignment-1/reference/hS4wB.png) | ![Running task list](design-alignment-1/app/task-list.png) |

**Pass.** The table is top-anchored and full-width. Group headers, columns, row separators, chips, task hierarchy, and monospaced identifiers follow the approved frame. The `UPDATED` cell remains an em dash until the engine publishes that fact, as recorded in ADR 0037.

### Task detail · Tela 4

| Approved frame `JQXi0` | Running Mac app |
| --- | --- |
| ![Approved task-detail frame](design-alignment-1/reference/JQXi0.png) | ![Running task detail](design-alignment-1/app/task-detail.png) |

**Pass.** The right-side sheet, title and metadata, description, protected boundary well, verification, subitems, attempts, comment well, and footer pills match the approved panel. Unbound preview controls are inert and hidden from accessibility under ADR 0038.

### New task · Tela 10

| Approved frame `ny4aa` | Running Mac app |
| --- | --- |
| ![Approved new-task frame](design-alignment-1/reference/ny4aa.png) | ![Running new-task dialog](design-alignment-1/app/new-task.png) |

**Pass.** The centered dialog, input wells, required boundary field, owner selector, option pills, keyboard hints, and two create actions follow the approved frame. Existing optional verification input remains available from the paperclip affordance under ADR 0039.

### Pane loop · Tela 6

| Approved frame `gSng7` | Running Mac app |
| --- | --- |
| ![Approved loop frame](design-alignment-1/reference/gSng7.png) | ![Running loop panel](design-alignment-1/app/loop.png) |

**Pass.** The anchored panel, prompt well, fixed idle rule, interval, round and stop controls, status lines, toggle, and hierarchy use the approved component. Explicit Apply and Cancel preserve staged editing under ADR 0040.

### Preferences · Tela 9

| Approved frame `fymXa` | Running Mac app |
| --- | --- |
| ![Approved preferences frame](design-alignment-1/reference/fymXa.png) | ![Running preferences panel](design-alignment-1/app/preferences.png) |

**Pass.** The centered 1080 × 700 panel, 220-point sidebar, page hierarchy, catalogue rows, selectors, wells, and shadows follow the approved frame. The running screen shows only the live installed catalogue entry. Uncontracted configuration affordances remain visual previews under ADR 0041.

## Recorded presentation boundaries

| ADR | Boundary kept during the presentation-only pass |
| --- | --- |
| 0036 | Inactive terminal panes preview engine-published facts while one selected terminal owns the live stream. |
| 0037 | The task-list update column shows an em dash until the engine publishes an update timestamp. |
| 0038 | Unbound task-detail controls are inert and absent from the accessibility control tree. |
| 0039 | Existing optional verification input remains available through the description attachment affordance. |
| 0040 | Loop changes remain staged behind Apply and Cancel; idleness stays an engine rule. |
| 0041 | Preferences reads live catalogue facts; settings without contracted persistence remain inert previews. |

## Pull requests

- [#98](https://github.com/soyeht/soyeht/pull/98) — canonical theme and bundled fonts.
- [#99](https://github.com/soyeht/soyeht/pull/99) — workspace, pane grid, new-pane entry, and agent pane.
- [#100](https://github.com/soyeht/soyeht/pull/100) — canonical typefaces across the workspace and conversation views.
- [#101](https://github.com/soyeht/soyeht/pull/101) — task board.
- [#102](https://github.com/soyeht/soyeht/pull/102) — task list and the Mac text-font gate.
- [#103](https://github.com/soyeht/soyeht/pull/103) — task detail.
- [#104](https://github.com/soyeht/soyeht/pull/104) — new-task dialog.
- [#105](https://github.com/soyeht/soyeht/pull/105) — pane loop controls.
- [#106](https://github.com/soyeht/soyeht/pull/106) — preferences.

## Verification

The final local gate passed with the real restricted vocabulary:

- 259 automated tests passed.
- 24 functional acceptances passed.
- 28 gate steps passed.
- Protocol generation was byte-stable.
- SQL outside the store: 0.
- Production panic sites: 0.
- Longest production function: 120 lines.
- Mac text using a raw system font: 0.
- No test, acceptance, or gate step was removed.
- `contracts/protocol.json`, engine, store, `README.md`, and `design/soyeht-desktop.pen`: no changes.

The running screenshots use live Dev-profile state. Content can differ from the static design data while the approved visual structure remains unchanged.
