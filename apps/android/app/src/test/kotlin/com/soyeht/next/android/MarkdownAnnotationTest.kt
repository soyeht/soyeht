package com.soyeht.next.android

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import com.soyeht.next.android.theme.Faces
import com.soyeht.next.android.ui.RunLook
import com.soyeht.next.android.ui.annotate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.soyeht_android.MarkdownRun

/** The runs the core hands over become styled text: what is drawn is what the shared vectors say. */
class MarkdownAnnotationTest {
    private val look = RunLook(accent = Color.Blue, codeBackground = Color.LightGray)

    private fun run(text: String, bold: Boolean = false, italic: Boolean = false, strike: Boolean = false, code: Boolean = false, link: String? = null) =
        MarkdownRun(text, bold, italic, strike, code, link)

    @Test
    fun styledRunsKeepTheirTextInOrderAndCarryTheirStyles() {
        val text = annotate(listOf(run("a "), run("b", bold = true), run(" "), run("c", italic = true), run(" "), run("d", strike = true)), look) {}
        assertEquals("a b c d", text.text)
        fun at(index: Int) = text.spanStyles.single { it.start <= index && index < it.end }.item
        assertEquals(FontWeight.Bold, at(2).fontWeight)
        assertEquals(FontStyle.Italic, at(4).fontStyle)
        assertEquals(TextDecoration.LineThrough, at(6).textDecoration)
    }

    @Test
    fun codeIsMonoOnTheWellColor() {
        val text = annotate(listOf(run("ls", code = true)), look) {}
        val style = text.spanStyles.single().item
        assertEquals(Faces.mono, style.fontFamily)
        assertEquals(Color.LightGray, style.background)
    }

    @Test
    fun aLinkOpensItsAddressInTheAccentAndUnderlined() {
        val opened = mutableListOf<String>()
        val text = annotate(listOf(run("docs", link = "https://example.com/a")), look) { opened += it }
        val link = text.getLinkAnnotations(0, text.length).single().item as LinkAnnotation.Url
        assertEquals("https://example.com/a", link.url)
        val style = link.styles?.style
        assertEquals(Color.Blue, style?.color)
        assertEquals(TextDecoration.Underline, style?.textDecoration)
        link.linkInteractionListener?.onClick(link)
        assertEquals(listOf("https://example.com/a"), opened)
    }

    @Test
    fun aHeadingIsBoldWhateverItsRunsSay() {
        val text = annotate(listOf(run("Title")), RunLook(Color.Blue, Color.LightGray, bold = true)) {}
        assertEquals(FontWeight.Bold, text.spanStyles.single().item.fontWeight)
    }

    @Test
    fun plainTextCarriesNoLinkAndNoDecoration() {
        val text = annotate(listOf(run("plain")), look) {}
        assertTrue(text.getLinkAnnotations(0, text.length).isEmpty())
        assertNull(text.spanStyles.single().item.textDecoration)
    }
}
