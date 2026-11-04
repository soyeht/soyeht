package com.soyeht.next.android

import androidx.compose.ui.graphics.Color
import com.soyeht.next.android.theme.Palettes
import com.soyeht.next.android.theme.resolve
import org.junit.Assert.assertEquals
import org.junit.Test
import uniffi.soyeht_android.TermColor

class TerminalColorsTest {
    private val palette = Palettes.milk
    private val foreground = Color(palette.ink)
    private val background = Color(palette.well)

    private fun resolved(color: TermColor) = resolve(color, palette, foreground, background)

    @Test
    fun theSixteenIndexedColorsAreTheThemesAnsiSlots() {
        for (index in 0 until 16) {
            assertEquals(Color(palette.ansi[index]), resolved(TermColor.Indexed(index.toUByte())))
        }
    }

    @Test
    fun theDefaultsAreTheThemesInkAndWell() {
        assertEquals(foreground, resolved(TermColor.DefaultForeground))
        assertEquals(background, resolved(TermColor.DefaultBackground))
    }

    @Test
    fun anRgbColorIsItself() {
        assertEquals(Color(1, 2, 3), resolved(TermColor.Rgb(1u, 2u, 3u)))
    }
}
