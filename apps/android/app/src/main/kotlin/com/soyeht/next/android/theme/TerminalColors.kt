package com.soyeht.next.android.theme

import androidx.compose.ui.graphics.Color
import uniffi.soyeht_android.TermColor

/**
 * A terminal cell color as a screen color. The core has already turned xterm's cube and grays into
 * RGB (the desktop client's function); what is left to the theme is the default pair and the sixteen
 * ANSI slots.
 */
fun resolve(color: TermColor, palette: Palette, defaultForeground: Color, defaultBackground: Color): Color =
    when (color) {
        is TermColor.DefaultForeground -> defaultForeground
        is TermColor.DefaultBackground -> defaultBackground
        is TermColor.Rgb -> Color(color.red.toInt(), color.green.toInt(), color.blue.toInt())
        is TermColor.Indexed -> Color(palette.ansi[color.index.toInt()])
    }
