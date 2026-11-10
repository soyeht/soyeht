package com.soyeht.next.android

import com.soyeht.next.android.theme.Eink
import com.soyeht.next.android.theme.Elevation
import com.soyeht.next.android.theme.PaneInterior
import com.soyeht.next.android.theme.Palettes
import com.soyeht.next.android.theme.Radius
import java.io.File
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** The generated tokens are the file they are generated from; `contracts/generate.py --check` holds the text. */
class DesignTokensTest {
    private val tokens = JSONObject(File(System.getProperty("soyeht.contracts"), "design-tokens.json").readText())

    private fun argb(hex: String) = 0xFF000000L or hex.removePrefix("#").toLong(16)

    @Test
    fun everyPaletteOfTheFileIsHereWithItsRoles() {
        val palettes = tokens.getJSONObject("palettes")
        // The order is the file's and `generate.py --check` holds it; a JSON object here has none.
        assertEquals(palettes.keys().asSequence().toSet(), Palettes.all.map { it.first }.toSet())
        for ((name, palette) in Palettes.all) {
            val source = palettes.getJSONObject(name)
            assertEquals(argb(source.getString("ink")), palette.ink)
            assertEquals(argb(source.getString("accent")), palette.accent)
            assertEquals(argb(source.getString("well")), palette.well)
            assertEquals(16, palette.ansi.size)
            assertEquals(source.getString("displayName"), palette.displayName)
        }
    }

    @Test
    fun theE_inkPaletteAndSizesAreTheFilesNot() {
        val source = tokens.getJSONObject("einkPalette")
        for (role in listOf("screen", "ink", "well", "accent", "border", "dim")) {
            val actual = when (role) {
                "screen" -> Palettes.eink.screen
                "ink" -> Palettes.eink.ink
                "well" -> Palettes.eink.well
                "accent" -> Palettes.eink.accent
                "border" -> Palettes.eink.border
                else -> Palettes.eink.dim
            }
            assertEquals(role, argb(source.getString(role)), actual)
        }
        assertEquals(16, Palettes.eink.ansi.size)
        assertEquals(tokens.getJSONObject("eink").getDouble("touchTarget").toFloat(), Eink.touchTarget)
        // It is not a theme the person picks.
        assertTrue(Palettes.all.none { it.second === Palettes.eink })
    }

    @Test
    fun theNumbersAreTheFilesNumbers() {
        assertEquals(tokens.getJSONObject("radius").getDouble("control").toFloat(), Radius.control)
        assertEquals(tokens.getJSONObject("paneInterior").getDouble("touchTarget").toFloat(), PaneInterior.touchTarget)
        assertTrue(PaneInterior.touchTarget >= 44f)
    }

    @Test
    fun aRaisedSurfaceTakesItsShadowsFromTheFile() {
        val pill = tokens.getJSONObject("elevation").getJSONObject("pill")
        assertEquals(pill.getDouble("offset").toFloat(), Elevation.pill.darkOffset)
        assertEquals(pill.getDouble("blur").toFloat(), Elevation.pill.lightBlur)
        val balloon = tokens.getJSONObject("elevation").getJSONObject("balloon")
        assertEquals(balloon.getDouble("lightOffset").toFloat(), Elevation.balloon.lightOffset)
        assertEquals(balloon.getDouble("darkBlur").toFloat(), Elevation.balloon.darkBlur)
    }
}
