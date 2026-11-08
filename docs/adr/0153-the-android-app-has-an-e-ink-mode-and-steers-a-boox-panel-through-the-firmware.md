# ADR 0153: The Android app has an e-ink mode, and steers a Boox panel through the firmware

- Status: accepted. Extends ADR 0134 (the Android app) and ADR 0013 (who sizes a terminal).
- Date: 2026-11-08

## Context

The owner reads and types on a Boox Note Air 3 C (Android 12L, a 10.3 inch Kaleido 3 colour panel, 1860 by 2480
at 300 dpi). The app as built for a phone looks wrong there. Fills differ from the page by about 1.2 to 1, so an
edge disappears; a modal has no scrim and no border; secondary text is 3.7 to 1 and muted 2.4 to 1, at 15 sp;
the ANSI colours wash out and colour-only meaning is lost; the terminal fills the top third of the screen with a
24 row grid at a phone's font; text lines run to 140 characters; and each output line repaints the whole text
block, which ghosts on a panel that refreshes slowly.

## Decision

**An e-ink mode.** It is on by itself when `Build.MANUFACTURER` is `ONYX`, and the person switches it by hand
(a pill on the pairing screen and on the list of panes); the choice is kept (`DisplaySettings`), so a person
who turns it off on a Boox is not switched back. In the mode:

- **The palette is `einkPalette` of `contracts/design-tokens.json`**, not an entry of `palettes`: it is not a
  theme the person picks, and the Swift and Rust theme lists, which are generated from `palettes`, stay as they
  were. It adds two roles the other palettes do not have: `border`, the edge of a card, well, pill or modal
  (a shadow does not show on sixteen greys), and `dim`, the ink of dimmed text, which is a lighter step and
  never an alpha. **It has no new colour**: every value is one that an existing palette already holds
  (the page is the `#FFFFFF` that `milk` has as `onAccent` and `shadowLight`; the ink is `misty blue`'s
  `#0C1012`; the greys, the accent and the ANSI colours are `milk`, `paleMist` and `mistyBlue` values). Page
  against ink is 19.1 to 1; secondary and muted are one colour at 9.8 to 1; the accent is solid with a white
  label (9.2 to 1); the page and four fills are five distinct grey levels of a sixteen-level panel; the ANSI
  colours 0 to 7 are eight distinct steps of luminance, each at least 3 to 1 on the page. A colour a program
  writes is moved towards ink until it reads on its ground (`legible`), because a program assumes a dark one.
- **The sizes are the `eink` section**, a section of the token file outside `desktop`. Only the Kotlin output
  carries it (as it does `einkPalette`): the Swift and Rust outputs do not, because a public name nothing
  reads fails `dead-public-swift:check`. Body text 18 sp, secondary 16 sp,
  terminal font 16 sp (growing to 28 sp, below), touch target 56 dp, a reading column of 640 dp, a border
  of 2 dp, the redraw interval (250 ms) and the settle delay (400 ms).
- **Nothing animates.** No ripple, no pressed indication, no overscroll glow, no fling, and the pickers are a
  popup that appears in one frame instead of Material's scaling menu.
- **Output is batched to about four redraws a second** (`redrawIntervalMs`), where it was one per 16 ms.
- **The cursor is a static block.** It never blinked in the terminal view; the mode keeps it so.

**The large screen.** Lists, forms and conversations are at most `readingColumnWidth` wide and centred; the
terminal takes the whole width. The terminal's size is the computer's (ADR 0013: a device that only opens a pane
does not send a size, and the pane keeps the size the person's Mac gave it). The phone therefore does not
change the grid: in the mode the **font grows until the columns fill the pane's width**, up to a maximum, and a
grid still wider scrolls sideways as before (`fitTextSize`). No protocol rule changes. Each row of the grid is a
layer that is drawn again only when its runs change, so a line of output marks one row of the screen, not the
whole text block.

**The panel's refresh.** A Boox lets an app choose the waveform a view is drawn with. The adapter
(`ScreenRefresh`) does something only when the mode is on and `Build.MANUFACTURER` is `ONYX`, and does nothing
anywhere else. On the terminal view the default is the text mode (REGAL, the vendor's partial update tuned for
text). Output that streams (two changed frames in a row) switches to the fast mode (ANIMATION, black and
white); about 400 ms after the last frame the fast mode ends and one full refresh (GC, sixteen greys) clears
the ghosts. A single changed frame, the echo of a key, is left to the text mode and does not flash. A "Refresh
screen" pill does the full refresh on demand.

**How the adapter calls the firmware: by reflection on two public methods, with no vendor library.**

- The vendor's documentation names `EpdController` of `onyxsdk-device`. That class exists only inside the
  library, not in the firmware, so it could be called by reflection only if the library were bundled anyway.
- `onyxsdk-device` 1.3.7 (the vendor's own Maven repository) declares **BSD-3** in its POM, and nothing
  else: the artifact carries no licence text and names no copyright holder, so the notice that BSD-3
  requires cannot be reproduced from it (ADR 0149). It also brings `fastjson2` and the Kotlin library, and
  a second Maven repository into the build, to make two calls. Its licence is therefore not clear enough, and
  it is not added.
- The firmware (read from the Note Air 3 C's `framework.jar`) adds to `android.view.View` the methods
  `setDefaultUpdateMode(int)` and `refreshScreen(int)`, which its API listing marks as SDK and test API: a normal
  app may call them and no hidden API is used. The adapter finds them by reflection, so the app has no
  compile-time dependency on the vendor; a firmware without them gives an adapter that does nothing, and a call
  that fails turns the adapter off. The mode numbers (REGAL 6, ANIMATION 4, GC 98) are the firmware's
  `UI_*_MODE` constants for the vendor's documented modes.
- The vendor's `ViewUpdateHelper` class (application-wide fast mode) is a hidden API, blocked to apps; it is not
  used and no bypass of the hidden API restriction is added.

## Consequences

- A person on a phone or an ordinary tablet sees nothing different, unless they switch the mode on: the palette
  and sizes are only used in the mode, and the adapter does nothing off a Boox.
- ANSI "bright" colours 9 to 14 are the same colour as 1 to 6 in the mode: eight steps of luminance are what
  the panel can tell apart. A program that says bright without bold loses that distinction.
- The mode numbers belong to the firmware, not to a published contract. A future firmware that renumbers them
  would give a different waveform until the numbers are updated; `BooxRefresh` holds them in one place.
- The refresh itself cannot be seen in a screenshot (`screencap` reads the framebuffer, not the panel); the
  owner judges it on the device.
