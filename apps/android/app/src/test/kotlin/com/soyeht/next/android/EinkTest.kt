package com.soyeht.next.android

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import com.soyeht.next.android.eink.OutputBurst
import com.soyeht.next.android.eink.ScreenRefresh
import com.soyeht.next.android.theme.Eink
import com.soyeht.next.android.theme.MINIMUM_CONTRAST
import com.soyeht.next.android.theme.Palettes
import com.soyeht.next.android.theme.contrast
import com.soyeht.next.android.theme.legible
import com.soyeht.next.android.ui.fitTextSize
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class EinkTest {
    private val palette = Palettes.eink
    private fun color(value: Long) = Color(value)

    @Test
    fun theE_inkPaletteKeepsTheContrastsTheBooxNeeds() {
        val page = color(palette.screen)
        assertTrue(contrast(color(palette.ink), page) >= 15f)
        assertTrue(contrast(color(palette.secondary), page) >= 7f)
        assertEquals(palette.secondary, palette.muted)
        assertTrue(contrast(color(palette.onAccent), color(palette.accent)) >= 7f)
        // The edge is what shows a card, a well, a pill or a modal: it reads on the page and on the well.
        assertTrue(contrast(color(checkNotNull(palette.border)), page) >= 7f)
        assertTrue(contrast(color(checkNotNull(palette.border)), color(palette.well)) >= 4.5f)
    }

    @Test
    fun theFillsStepAwayFromThePageInDistinctGreys() {
        val steps = listOf(palette.screen, palette.canvas, palette.hover, palette.well, palette.selection).map { color(it).luminance() }
        // Each fill is darker than the one before it: five levels, the page and four fills.
        assertEquals(steps.sortedDescending(), steps)
        assertEquals(steps.size, steps.toSet().size)
        assertTrue(contrast(color(palette.screen), color(palette.well)) >= 2f)
    }

    @Test
    fun theFirstEightAnsiColorsAreDistinctStepsOfLuminanceAndReadOnThePage() {
        val page = color(palette.screen)
        val steps = palette.ansi.take(8).map { color(it).luminance() }.sorted()
        assertTrue(steps.zipWithNext().all { (low, high) -> high - low >= 0.02f })
        assertTrue(palette.ansi.take(8).all { contrast(color(it), page) >= 3f })
    }

    @Test
    fun dimTextIsALighterStepAndNotAnAlpha() {
        val dim = color(checkNotNull(palette.dim))
        assertEquals(1f, dim.alpha)
        assertTrue(dim.luminance() > color(palette.ink).luminance())
        assertTrue(contrast(dim, color(palette.screen)) >= 4f)
    }

    @Test
    fun onlyTheE_inkPaletteHasAnEdge() {
        assertTrue(Palettes.all.all { (_, other) -> other.border == null && other.dim == null })
        assertNotNull(palette.border)
    }

    @Test
    fun theE_inkSizesMeetTheirTargets() {
        assertTrue(Eink.bodyTextSize >= 18f)
        assertTrue(Eink.captionTextSize >= 16f)
        assertTrue(Eink.terminalFontSize in 14f..16f)
        assertEquals(56f, Eink.touchTarget)
        assertTrue(Eink.readingColumnWidth > 0f)
        assertTrue(Eink.borderWidth > 0f)
    }

    @Test
    fun theModeIsOnForABooxUntilThePersonSwitchesItOff() {
        assertTrue(einkEnabled(null, "ONYX"))
        assertFalse(einkEnabled(null, "samsung"))
        assertFalse(einkEnabled(false, "ONYX"))
        assertTrue(einkEnabled(true, "Google"))
    }

    @Test
    fun theFontGrowsToFillTheWidthAndNeverBelowTheBase() {
        // 80 columns of 10 px cells at 16 px fill 800 px; 1600 px of width doubles the font.
        assertEquals(32f, fitTextSize(80, 1600f, 16f, 40f, 10f))
        assertEquals(40f, fitTextSize(80, 4000f, 16f, 40f, 10f))
        assertEquals(16f, fitTextSize(80, 400f, 16f, 40f, 10f))
        // Where the base is the largest size (the standard mode) the grid is left as it is.
        assertEquals(16f, fitTextSize(80, 1600f, 16f, 16f, 10f))
        assertEquals(16f, fitTextSize(0, 1600f, 16f, 40f, 10f))
    }

    @Test
    fun aColorThatWouldVanishIsMovedUntilItReads() {
        val page = color(palette.screen)
        val ink = color(palette.ink)
        val pale = Color(0xFFFFFF99)
        val moved = legible(pale, page, ink, page)
        assertTrue(contrast(moved, page) >= MINIMUM_CONTRAST)
        // A color that already reads is left alone.
        assertEquals(ink, legible(ink, page, ink, page))
        // On a dark ground the move is towards the page.
        assertTrue(contrast(legible(ink, ink, ink, page), ink) >= MINIMUM_CONTRAST)
    }

    private class Recorder : ScreenRefresh {
        val calls = mutableListOf<String>()
        override val available = true
        override fun textMode() { calls += "text" }
        override fun fastMode() { calls += "fast" }
        override fun fullRefresh() { calls += "full" }
    }

    @Test
    fun oneFrameIsLeftToTheViewsModeAndAStreamEndsInAFullRefresh() {
        val recorder = Recorder()
        val burst = OutputBurst(recorder)
        burst.frame()
        burst.quiet()
        assertEquals(emptyList<String>(), recorder.calls)
        burst.frame()
        burst.frame()
        burst.frame()
        assertEquals(listOf("fast"), recorder.calls)
        burst.quiet()
        assertEquals(listOf("fast", "text", "full"), recorder.calls)
        // The next burst starts over.
        burst.frame()
        burst.quiet()
        assertEquals(3, recorder.calls.size)
    }
}
