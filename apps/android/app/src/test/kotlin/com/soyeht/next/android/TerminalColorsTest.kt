package com.soyeht.next.android

import androidx.compose.ui.graphics.Color
import com.soyeht.next.android.theme.Palettes
import com.soyeht.next.android.theme.indexed
import org.junit.Assert.assertEquals
import org.junit.Test

class TerminalColorsTest {
    private val palette = Palettes.milk

    @Test
    fun theFirstSixteenColorsAreTheThemesAnsiSlots() {
        for (index in 0 until 16) assertEquals(Color(palette.ansi[index]), indexed(index, palette))
    }

    @Test
    fun theNextTwoHundredSixteenAreTheXtermColorCube() {
        assertEquals(Color(0, 0, 0), indexed(16, palette))
        assertEquals(Color(0, 0, 95), indexed(17, palette))
        assertEquals(Color(255, 0, 0), indexed(196, palette))
        assertEquals(Color(255, 255, 255), indexed(231, palette))
    }

    @Test
    fun theLastTwentyFourAreTheGrays() {
        assertEquals(Color(8, 8, 8), indexed(232, palette))
        assertEquals(Color(238, 238, 238), indexed(255, palette))
    }
}
