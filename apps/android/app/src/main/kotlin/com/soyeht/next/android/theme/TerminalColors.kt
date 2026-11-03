package com.soyeht.next.android.theme

import androidx.compose.ui.graphics.Color
import uniffi.soyeht_android.TermColor

/**
 * A terminal cell color as a screen color. The 16 ANSI slots are the theme's; the next 216 are the
 * xterm color cube and the last 24 its grays, which no theme changes.
 */
fun resolve(color: TermColor, palette: Palette, defaultForeground: Color, defaultBackground: Color): Color =
    when (color) {
        is TermColor.DefaultForeground -> defaultForeground
        is TermColor.DefaultBackground -> defaultBackground
        is TermColor.Rgb -> Color(color.red.toInt() and 0xFF, color.green.toInt() and 0xFF, color.blue.toInt() and 0xFF)
        is TermColor.Indexed -> indexed(color.index.toInt() and 0xFF, palette)
    }

fun indexed(index: Int, palette: Palette): Color = when {
    index < 16 -> Color(palette.ansi[index])
    index < 232 -> {
        val cube = index - 16
        fun level(step: Int) = if (step == 0) 0 else 55 + 40 * step
        Color(level(cube / 36), level(cube / 6 % 6), level(cube % 6))
    }
    else -> {
        val gray = 8 + 10 * (index - 232)
        Color(gray, gray, gray)
    }
}
