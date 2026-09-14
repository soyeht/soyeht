# ADR 0065 · Neomorphic theme boundaries

**Status:** Accepted · 2026-09-14

## Context

The rewrite currently exposes the approved Neo · Milk look through fixed SwiftUI colors. Production Soyeht has nine reviewed neomorphic themes. Each theme owns its canvas, raised face, recessed well, ink, accent, semantic colors, agent plates, and separate raised and well shadow pairs. Treating any missing role as permission to derive a replacement would silently restyle an approved theme.

## Decision

`TerminalColorTheme` is a closed client-side value with the nine production identifiers and display names. It selects a deterministic `SoyehtThemePalette` in the shared client package. The catalogue ports the reviewed color data; `ThemeHexColorMath`, `ThemeLabColorMath`, and `ThemeShadowMath` provide the common deterministic arithmetic. Per-theme raised and well pairs remain authored data. Dark appearance is classified from the canvas with perceptual Lab lightness, matching production.

Neo · Milk remains the seeded default. Its rewrite-specific editor roles are pinned to the values visible before this slice. The other themes map those roles to their authored bright ANSI semantic slots, so every rendered color still comes from the selected palette.

The active theme is client-local state because this is the smallest surface that provides one remembered app-wide choice. A single observable store in the shared client package will read and write one preference key. The Mac and iPhone app roots will inject that same store instance into their view trees. Switching the value changes the palette observed by every open view immediately; restarting reconstructs it from the preference. No engine method, database row, or wire type is added, and cross-device synchronization stays outside this slice.

`SoyehtThemeColors` and `MacTheme` remain the only view-facing color sources. They will become projections of the observed active palette. Existing raised and well modifiers keep their geometry and top-left light direction, but use the palette's separate outer and inner pairs; dark wells may also draw their authored lip.

The color gate remains closed. It continues to reject literals and direct system colors in both app trees, and adds a catalogue check that names all nine cases, validates every hex role, and exercises the default and a dark theme. Adding a theme requires adding a closed enum case, a complete palette, and parity expectations. Widening the gate to accept arbitrary colors is not an implementation path.

## Consequences

- Production's nine reviewed palettes become explicit immutable data in the rewrite.
- A missing or malformed role is a test failure rather than a runtime fallback.
- Live switching remains a client concern and does not change protocol version 1.
- All existing surfaces keep one style API while receiving the active theme.
- Per-pane themes, user-authored themes, synchronization, and animated transitions remain outside this slice.
