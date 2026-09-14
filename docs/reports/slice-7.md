# Slice 7 · neomorphic themes

Date: 2026-09-14. Profile: `Soyeht.next/dev`. License: Apache-2.0. Implementation
head before this report: `914560bbd9e82d9d6503a2d5d64376d030c7c26e`.

Slice 7 is complete. The rewrite now carries the same closed set of nine
neomorphic themes as production Soyeht, with Neo · Milk as the seeded default.
The active theme is one client-local value that drives every Mac and iPhone
color, applies to all open views immediately and survives an app restart. No
engine, store or protocol surface changed, and no tag or release was created.

## Delivered scope

ADR 0065 fixed the boundary before implementation. `TerminalColorTheme` is a
closed value in the shared client package. It selects one complete immutable
`SoyehtThemePalette`; `SoyehtThemeColors` and `MacTheme` project view-facing
tokens from the active palette. Views never select or construct colors.

The catalogue ports the reviewed production data rather than deriving missing
roles. Every theme owns canvas, raised face, hover, well, ink, secondary and
muted text, accent, selection, semantic and syntax colors, agent plates, ANSI
colors, and separate raised and well shadow pairs. The four dark themes also
carry the authored dark-well lip. Lab lightness determines `isDark`, matching
the production classification. Hex, Lab and shadow arithmetic are deterministic
pure helpers.

`SoyehtThemeStore` is the single observable source of the active theme. Its
`ThemePreferenceStore` port has a UserDefaults adapter and an in-memory test
double. Missing or unknown persisted identifiers fail closed to Neo · Milk.
The Mac and iPhone app roots own one store each and set the preferred color
scheme from the selected palette. Terminal, code-editor and writing native
surfaces refresh their attributed content when the theme changes, without
recreating panes or involving the engine.

Preferences → Appearance lists all nine themes in the production order and
uses each candidate palette to paint its own raised and recessed preview. A
selection is applied immediately, marked accessibly and persisted through the
same store.

The color gate remains closed. It scans both app trees for literal or direct
system colors and also verifies the closed catalogue, every required palette
role, all nine enum cases, the seeded default projection and a dark-theme
exercise. Its red control proves that a literal, an incomplete projection and
a missing theme are rejected; its green control proves that a complete
palette-driven implementation passes.

## Theme catalogue

These primary roles are authored data shared with the production themes. The
full catalogue also pins the raised/well pairs, semantic colors, agent plates
and ANSI slots.

| Theme | Canvas | Face | Accent | Appearance |
| --- | --- | --- | --- | --- |
| Neo · Milk | `#E0E5EC` | `#E8EDF4` | `#5B7CFA` | light |
| Neo · Pale Mist | `#B8C0C2` | `#C0C8CA` | `#0F64D1` | light |
| Neo · Misty Blue | `#7B9FAC` | `#83A7B4` | `#004497` | light |
| Neo · Sunlit Chartreuse | `#D2CE67` | `#DCD870` | `#B55C03` | light |
| Neo · Sunrise Gold | `#E4B15C` | `#EDB964` | `#A84500` | light |
| Neo · Deep Vine | `#32341E` | `#3B3D26` | `#F5C64C` | dark |
| Neo · Deep Forest | `#1A341B` | `#223D22` | `#B2D353` | dark |
| Neo · Midnight Teal | `#00202E` | `#062635` | `#7FA7FF` | dark |
| Neo · Deep Harbor | `#223F48` | `#2B4851` | `#19B2FF` | dark |

The Neo · Milk golden test pins the complete pre-slice token set byte for byte.
The reviewer independently confirmed Milk face `#E8EDF4` and accent `#5B7CFA`,
and Pale Mist canvas `#B8C0C2`, accent `#0F64D1` and selection `#99B2CC`
against `NeoPanePresets` and `DesignStylePresets` in the read-only production
source.

## Acceptance evidence

| Acceptance | Result | Verification |
| --- | --- | --- |
| 1 · boundary | passed | ADR 0065 records the theme-value → palette → tokens boundary, client-local persistence and the catalogue-aware color gate. It landed before the feature PRs. |
| 2 · palette parity | passed | All nine complete palettes compute. Tests pin the Neo · Milk golden, every raised and well shadow pair, Lab-derived `isDark`, hex/Lab round trips and shadow geometry against production values. |
| 3 · no literal colors | passed | The Mac/iPhone color scan and its red/green controls pass. Every app color projects from the active palette and all nine catalogue entries are required. |
| 4 · live switching and persistence | passed | The reviewer changed the running app to Midnight Teal and observed tabs, rail, Preferences and background panes re-render immediately. A close/relaunch restored the selected theme before any new selection; store tests cover default, unknown identifiers, all nine selections and restart restoration. |
| 5 · picker | passed | Preferences renders all nine exact names and candidate-colored neomorphic previews from `TerminalColorTheme.allCases`; selecting a card updates the shared store and accessibility state. |
| 6 · design fidelity | passed, reviewer confirmed | The reviewer confirmed the closing Dev build against production. Real framebuffer pairs for every theme are below; Milk shows the light raised/well recipe and Deep Harbor shows the authored dark-well recipe. |
| 7 · no regression | passed | 507 repository test declarations, 37 automated acceptances and all 35 gate steps pass. Protocol generation, four hardening ratchets, font/color gates, restricted-vocabulary scan, Rust, Mac and iPhone checks are green; protocol version remains 1. |

## Theme comparisons

Every image below is a real macOS framebuffer capture. The left side is the
installed production Soyeht app wearing the named production theme; the right
side is the signed closing Dev build at `914560b` wearing the ported theme.
Workspace content differs, while the compared canvas, face, ink, accents,
raised shadows and recessed wells are the named theme's own authored values.
The production preference was restored to its original Neo · Deep Harbor after
capture; the Dev build was restored to seeded Neo · Milk.

The picker itself shows the closed catalogue and theme-specific previews:

![Closing Appearance picker with all nine themes](slice-7/rewrite/picker.png)

### Neo · Milk

| Production | Rewrite Dev |
| --- | --- |
| ![Production Neo Milk](slice-7/production/milk.png) | ![Rewrite Neo Milk](slice-7/rewrite/milk.png) |

**Pass.** The light canvas, raised face, white top-left light, blue accent and
recessed control pair match. This is the byte-pinned default.

### Neo · Pale Mist

| Production | Rewrite Dev |
| --- | --- |
| ![Production Neo Pale Mist](slice-7/production/pale-mist.png) | ![Rewrite Neo Pale Mist](slice-7/rewrite/pale-mist.png) |

**Pass.** The neutral mist canvas, cool face, blue accent and selection family
match the production preset.

### Neo · Misty Blue

| Production | Rewrite Dev |
| --- | --- |
| ![Production Neo Misty Blue](slice-7/production/misty-blue.png) | ![Rewrite Neo Misty Blue](slice-7/rewrite/misty-blue.png) |

**Pass.** Blue canvas and face, dark ink and paired blue-gray shadows match.

### Neo · Sunlit Chartreuse

| Production | Rewrite Dev |
| --- | --- |
| ![Production Neo Sunlit Chartreuse](slice-7/production/sunlit-chartreuse.png) | ![Rewrite Neo Sunlit Chartreuse](slice-7/rewrite/sunlit-chartreuse.png) |

**Pass.** Chartreuse canvas, warm ink, orange accent and tinted light/shadow
pairs match.

### Neo · Sunrise Gold

| Production | Rewrite Dev |
| --- | --- |
| ![Production Neo Sunrise Gold](slice-7/production/sunrise-gold.png) | ![Rewrite Neo Sunrise Gold](slice-7/rewrite/sunrise-gold.png) |

**Pass.** Gold canvas, orange accent and warm raised/recessed surfaces match.

### Neo · Deep Vine

| Production | Rewrite Dev |
| --- | --- |
| ![Production Neo Deep Vine](slice-7/production/deep-vine.png) | ![Rewrite Neo Deep Vine](slice-7/rewrite/deep-vine.png) |

**Pass.** The first dark theme uses light ink, gold accent and the authored
three-part dark-well treatment, including its separate lip.

### Neo · Deep Forest

| Production | Rewrite Dev |
| --- | --- |
| ![Production Neo Deep Forest](slice-7/production/deep-forest.png) | ![Rewrite Neo Deep Forest](slice-7/rewrite/deep-forest.png) |

**Pass.** Deep green canvas, chartreuse accent and dark raised/well pairs match.

### Neo · Midnight Teal

| Production | Rewrite Dev |
| --- | --- |
| ![Production Neo Midnight Teal](slice-7/production/midnight-teal.png) | ![Rewrite Neo Midnight Teal](slice-7/rewrite/midnight-teal.png) |

**Pass.** The reviewer exercised this theme live. The entire app updated at
once to the teal canvas, blue accent, light ink and authored dark wells.

### Neo · Deep Harbor

| Production | Rewrite Dev |
| --- | --- |
| ![Production Neo Deep Harbor](slice-7/production/deep-harbor.png) | ![Rewrite Neo Deep Harbor](slice-7/rewrite/deep-harbor.png) |

**Pass.** Harbor canvas and face, cyan accent, light ink and the dark well's
shadow, rim and lip match. A close/relaunch of the Dev app restored this theme,
providing the live restart evidence before the build was returned to Milk.

## Local merge gate

Hosted GitHub Actions remains disabled by project decision. Every PR used the
local gate with the private restricted vocabulary and the implementation base.
The final implementation gate passed:

- 507 repository test declarations;
- 37 automated acceptances;
- all 35 gate steps, with no removed test, acceptance or step;
- generated protocol and profile files with no diff;
- protocol version 1;
- Rust formatting, clippy with warnings as errors, build and workspace tests;
- shared-client tests and Mac and iPhone builds;
- launchd end-to-end suites for slices 1–6;
- all measured editor performance budgets;
- zero SQL outside the store;
- zero production panic sites;
- zero production functions above 120 lines, with the maximum at 120; and
- zero raw Mac text fonts or unapproved Mac/iPhone colors.

The report PR repeats the complete gate from the final implementation base.

## Run from scratch on a clean Mac

1. Install the pinned toolchains and Xcode, clone the repository and create a
   dedicated worktree. Run `scripts/check_all.sh` with `FORBIDDEN_TERMS`
   supplied through the environment.
2. Build and open the signed Dev Mac app. Neo · Milk must be selected when no
   recognized theme preference exists.
3. Open Preferences → Appearance and confirm all nine names and preview
   swatches appear in the closed production order.
4. Keep terminal, writing and agent panes open. Select each light and dark
   theme and confirm every open surface updates immediately without pane or
   engine lifecycle changes.
5. Select a dark theme, close and reopen the app, and confirm it is restored.
   Return the Dev profile to Neo · Milk when the check is complete.

## Decisions and limits

- [ADR 0065](../adr/0065-neomorphic-theme-boundaries.md) records the closed
  catalogue, authored shadow roles, palette projection, client-local store and
  upgraded color gate.
- The active theme is app-wide and local to the client. Per-pane, per-workspace,
  synchronized and user-authored themes remain outside this slice.
- Theme changes affect presentation only. Pane identity, engine state, file
  ownership, protocol behavior and stored product data are unchanged.

## Delivery pull requests

1. [#237 · slice objective](https://github.com/soyeht/soyeht/pull/237)
2. [#238 · palette core and boundaries](https://github.com/soyeht/soyeht/pull/238)
3. [#239 · active palette tokens and color gate](https://github.com/soyeht/soyeht/pull/239)
4. [#240 · persisted live theme store](https://github.com/soyeht/soyeht/pull/240)
5. [#241 · Appearance theme picker](https://github.com/soyeht/soyeht/pull/241)

The report PR records the final gates and reviewer-confirmed visual evidence for
slice 7.
