package com.soyeht.next.android.theme

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.luminance
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

/** The contrast ratio of two colors, as WCAG defines it: 1 for the same color, 21 for black on white. */
fun contrast(first: Color, second: Color): Float {
    val a = first.luminance()
    val b = second.luminance()
    return (maxOf(a, b) + LUMINANCE_OFFSET) / (minOf(a, b) + LUMINANCE_OFFSET)
}

/**
 * [color] read on [background]: itself when it keeps [minimum] contrast, else moved towards [ink] on a light
 * ground or [page] on a dark one until it does, so that its hue survives as far as the contrast allows. A program
 * writes true colors for the dark ground it assumes, and a Boox's page is white.
 */
fun legible(color: Color, background: Color, ink: Color, page: Color, minimum: Float = MINIMUM_CONTRAST): Color {
    if (contrast(color, background) >= minimum) return color
    val towards = if (background.luminance() > MIDDLE_LUMINANCE) ink else page
    for (step in 1 until LEGIBLE_STEPS) {
        val moved = lerp(color, towards, step / LEGIBLE_STEPS.toFloat())
        if (contrast(moved, background) >= minimum) return moved
    }
    return towards
}

/** WCAG's AA contrast for text. */
const val MINIMUM_CONTRAST = 4.5f
private const val LUMINANCE_OFFSET = 0.05f
private const val MIDDLE_LUMINANCE = 0.18f
private const val LEGIBLE_STEPS = 16
