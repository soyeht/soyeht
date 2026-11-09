package com.soyeht.next.android.ui

/**
 * The size, in pixels, at which [columns] cells of a mono face fill [availablePx]. The phone never resizes the
 * computer's pane (ADR 0013), so a large screen fills its width with a larger font instead. It never goes below
 * [basePx]: a grid wider than the screen at that size scrolls sideways, as it did. [cellWidthAtBase] is the
 * width of one cell at [basePx], and [maxPx] is the largest size the font may grow to.
 */
fun fitTextSize(columns: Int, availablePx: Float, basePx: Float, maxPx: Float, cellWidthAtBase: Float): Float {
    if (columns <= 0 || cellWidthAtBase <= 0f) return basePx
    val fitted = basePx * availablePx / (cellWidthAtBase * columns)
    return fitted.coerceIn(basePx, maxOf(basePx, maxPx))
}
