# iPhone screenplay page and pinch

Date: 2026-09-20

Base tree: `69ef363` (block D2, PR #372)

Scope: block D3. This change replaces the iPhone screenplay placeholder with a
fixed Courier Prime page, uniform whole-page fitting, editing through the shared
writer, and native bounded pinch/pan. Presentation and explicit companion tabs
remain D4.

## Geometry and typography

`PhoneWritingLayoutPolicy` is the source of both the unscaled page size and the
fit scale. The page width is `61 * active Courier glyph width + 2 * 74`, and its
height is exactly `11 / 8.5` times that width. The fit is the smaller of the
available width and height ratios, clamped at the measured 7.5 pt raster floor.
The SwiftUI layer adds no measurement pass or per-leaf `GeometryReader`.

The iOS renderer and the floor calculation both read the bundled Courier Prime
16 pt font through `PhoneWritingMetrics.screenplayFont`. There is no second
literal font metric in the view. With the accepted fixture metric, the base page
is 733.219 x 948.872 pt; the 345 pt content width at the 375 pt ruler fits it at
about 47.05 percent and preserves 8.5:11 without reflow.

## Gesture and local state

One native `UIScrollView` owns magnification. Its minimum is the pure fit scale
and its maximum is four times that fit. UIKit keeps the pinch centroid stable and
constrains pan to the scaled page. At the minimum scale the page is centered and
has no horizontal scrolling; the inner screenplay text view may scroll
vertically. Above the minimum, the outer view owns pan in both axes.

At the end of a pinch, the view stores only the multiplier over the current fit
through the D1 `setScreenplayZoom` action. That value is device-local and scoped
to engine, workspace, and pane identity. Rotation recomputes the fit while
preserving the multiplier. Collapsing unmounts the heavy view after draining the
shared edit queue; reopening restores the same multiplier.

## Editing and lifecycle

The page consumes the existing bounded editor viewport and writing structure.
It uses `ScreenplayInteraction.typingEdit` and
`EngineClient.enqueueEditorEdits`, so there is no second writer or protocol
path. Fountain element styling uses the same structure facts as the Mac view.
The page is loaded only when its leaf is expanded; a collapsed screenplay does
not retain the UIKit editor.

## Automated evidence

The focused `PhoneWriting` run contains 18 passing tests. D3 adds assertions for
the fixed ratio, both fit axes, page size at every accepted phone matrix point,
and the absence of a screenplay page on text leaves. The existing action/store
tests cover zoom bounds and persistence.

Validation commands are run without output pipes:

```sh
swift test --package-path apps/shared --filter PhoneWriting
bash scripts/check_ios.sh
bash scripts/check_mac.sh
SOYEHT_DEVELOPMENT_TEAM=ABCDE12345 bash scripts/check_ios.sh --sign
```

Physical installation and pinch/edit/relaunch evidence are added to the PR when
the dedicated `iPhone Devs` is available. `iPhone Caio` is not a substitute.
