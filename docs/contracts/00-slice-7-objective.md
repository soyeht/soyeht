# Objective · New Soyeht slice 7: neomorphic themes (the same theme set as production Soyeht)

Self-contained, for execution to completion without questions to Caio. Same absolute safeguards as slices 1–6: no production, Dev profile Soyeht.next/dev, no secrets, no tags/releases, no competitor brand names in product docs, English-only repo, Apache-2.0, no AI attribution. Autonomy rule: ambiguity becomes an ADR and work continues; self-merge with the green local gate; victoria reviews each PR after merge. Do ALL git in your own clone, NEVER the main clone (design/.pen only). Date: 2026-09-14.

## 0. Why + the READ-ONLY spec

Caio (testing slice 6): the rewrite must ship the SAME neomorphic themes the production Soyeht app already has — not one hard-coded palette. The neomorphic style is non-negotiable ("luz e sombra"). The production theme system is the source of truth and is READ-ONLY — ~/Documents/SwiftProjects/iSoyehtTerm is untouchable; read it, never edit it. Port its themes into the rewrite's own architecture (do not vendor its code).

Production theme sources to port from (read-only): ~/Documents/SwiftProjects/iSoyehtTerm/Packages/SoyehtCore/Sources/SoyehtCore/Theme/ — Neo/NeoPalette.swift (per-theme palette), NeoStyleColors.swift, SoyehtAppPalette.swift (incl. isDark perceptual lightness), DesignStyle.swift + DesignStylePresets.swift, NeoPanePresets.swift, HexColorMath.swift, LabColorMath.swift, NeoShadowMath.swift, the TerminalColorTheme type and TerminalThemeStore (active theme + persistence), and iSoyehtTerm/.claude/skills/neomorphism-macos/SKILL.md (light from top-left, solid tinted shadow pairs, blur ≈ 1.7–2× offset scaled by surrounding space, radius card 20 / control 12 / pill 999, pressed = same box with an inner well pair, no outer shadow).

The nine themes (exact display names): Neo · Milk, Neo · Pale Mist, Neo · Misty Blue, Neo · Sunlit Chartreuse, Neo · Sunrise Gold, Neo · Deep Vine, Neo · Deep Forest, Neo · Midnight Teal, Neo · Deep Harbor. Some light, some dark (isDark); the shadow math adapts per theme (dark faces use the dark-well recipe). Neo · Milk is the current default and must remain the seeded default.

## Architecture and maintainability (decided first, before any feature)

The first PR is a boundary ADR. It must state, and every later PR must hold:

1. The theme is a value that drives every color. A TerminalColorTheme (ThemeId ported 1:1 from production) selects a computed palette; SoyehtThemeColors / MacTheme derive ALL colors and the neomorphic shadow pairs from that palette — no literal colors anywhere in apps/mac or apps/ios. Port the palette computation and the Lab/Hex shadow math so each theme's raised/well pairs match production, light and dark.
2. One theme store; live switching; persisted. A single source of active theme, persisted so it survives restart, switchable at runtime with every open pane/view updating live. Decide client-local vs engine-owned persistence in the ADR and take the SMALLEST surface that meets "one active theme per user, remembered": client-local settings → no wire change; engine/store → the wire is ADDITIVE and protocol version stays 1 (generate.py --check green).
3. The color gate is upgraded, not bypassed. Today the Mac/iPhone color ratchet enforces approved theme colors. With nine themes it must enforce that every color resolves through the active theme palette (no literal colors), and that all nine palettes are present and self-consistent — keep the ratchet meaningful, never widen it into "any color allowed."
4. Bounded + additive. The theme set is a closed, enumerated list; nothing in slices 1–6 changes behaviourally; the writing surfaces, browser, editor, tasks and agent panes all re-render correctly under every theme.
5. Reuse, don't reinvent the pressed-control / well recipe already in MacStyle.swift (neoRaised, elevation light/dark pairs) — extend it to be theme-driven rather than replacing it.

## 1. Order of work

1.1 Boundary ADR + palette core: port TerminalColorTheme + the per-theme NeoPalette computation + the Lab/Hex shadow math into the rewrite as a bounded deterministic core; prove all nine palettes compute and that Neo · Milk reproduces today's exact values.
1.2 Theme-drive SoyehtThemeColors / MacTheme: every token derives from the active palette; remove any remaining literals; the four hardening ratchets + font/color gates green.
1.3 Theme store + live switching (persisted, restart-safe).
1.4 Theme picker in Preferences (the approved preferences surface): list the nine themes with a live neomorphic preview swatch and the display names, matching production's picker; switch applies immediately.

## 2. Acceptance (work ends only when all pass)

1. [local/ADR] The theme-value → palette → tokens boundary, the persistence choice, and the color-gate upgrade are recorded in an ADR.
2. [CI] Palette parity. All nine themes compute; Neo · Milk equals the pre-slice values byte-for-byte (golden test); each theme's raised and well shadow pairs and isDark match the production computation for the same theme.
3. [CI] No literal colors. The color gate proves every apps/mac + apps/ios color resolves through the active theme; zero literals; all nine palettes present.
4. [CI] Live switching + persistence. Switching the active theme updates every open pane live and the choice survives an engine/app restart.
5. [CI] Picker. Preferences lists the nine themes with previews and display names and applies a selection immediately.
6. [reviewer] Design fidelity. victoria compares each rewrite theme, rendered in the Dev app, against the SAME production theme (side-by-side) — face/canvas/ink, the raised and pressed/well shadow pairs, radii and accents must match — and pastes the pairs in docs/reports/slice-7.md.
7. [CI] No regression. Every slice 1–6 acceptance/test passes under the default theme and spot-checked under a dark theme; four ratchets + font/color gates + restricted-vocabulary scan green; generate.py --check clean; protocol version still 1.

## 3. Outside this slice

Per-pane or per-workspace themes (one active theme app-wide, matches production); custom user-authored themes / a theme editor; cross-device theme sync unless it falls out of the persistence choice; animated theme transitions. Keep the set to the nine production themes.

## 4. Workflow and delivery

Small PRs (palette core, token drive, store, picker), green local gate, self-merge, an ADR per ambiguity. On completion, docs/reports/slice-7.md with per-theme image pairs (rewrite vs production) and the ADR link, and a message to victoria with the PR links. victoria reviews each PR for correctness, contract preservation, per-theme design fidelity vs production, and color-gate integrity (no literals, all nine palettes, ratchet still discriminating).
