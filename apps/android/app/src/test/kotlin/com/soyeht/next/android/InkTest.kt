package com.soyeht.next.android

import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import com.soyeht.next.android.ink.Handwriting
import com.soyeht.next.android.ink.HandwritingChoice
import com.soyeht.next.android.ink.HandwritingLanguage
import com.soyeht.next.android.ink.ModelState
import com.soyeht.next.android.ink.InkOutcome
import com.soyeht.next.android.ink.InkPoint
import com.soyeht.next.android.ink.InkSession
import com.soyeht.next.android.ink.InkStroke
import com.soyeht.next.android.ink.StrokeShapes
import com.soyeht.next.android.ink.TextEdits
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class InkTest {
    private fun stroke(vararg xy: Pair<Float, Float>, start: Long = 0L, step: Long = 10L) =
        InkStroke(xy.mapIndexed { i, (x, y) -> InkPoint(x, y, start + i * step) })

    /** A scribble: back and forth across [width], [turns] times, in a band [height] tall. */
    private fun scribble(x: Float, y: Float, width: Float, turns: Int, height: Float = 10f): InkStroke {
        val points = (0..turns).map { if (it % 2 == 0) x to y else x + width to y + height }
        return stroke(*points.toTypedArray())
    }

    @Test
    fun aShortQuickTouchIsATapAndAStrokeIsNot() {
        assertTrue(StrokeShapes.isTap(stroke(10f to 10f, 11f to 11f), slop = 12f))
        assertFalse(StrokeShapes.isTap(stroke(10f to 10f, 80f to 10f), slop = 12f))
        // A touch that stays but takes long is not a tap.
        assertFalse(StrokeShapes.isTap(stroke(10f to 10f, 11f to 10f, start = 0L, step = 400L), slop = 12f))
    }

    @Test
    fun aBackAndForthScribbleIsAScratchOutAndWritingIsNot() {
        assertTrue(StrokeShapes.isScratchOut(scribble(0f, 0f, 120f, turns = 6), unit = 2f))
        // A straight line, a loop and a short wiggle are writing.
        assertFalse(StrokeShapes.isScratchOut(stroke(0f to 0f, 120f to 0f), unit = 2f))
        assertFalse(StrokeShapes.isScratchOut(stroke(0f to 0f, 60f to 40f, 120f to 0f, 60f to -40f, 0f to 0f), unit = 2f))
        assertFalse(StrokeShapes.isScratchOut(scribble(0f, 0f, 8f, turns = 6), unit = 2f))
    }

    @Test
    fun aScribbleOverTheWritingTakesItAwayAndOneOverNothingDeletesAWord() {
        val session = InkSession(unit = 2f)
        assertEquals(InkOutcome.Written, session.finish(stroke(0f to 0f, 40f to 30f)))
        assertEquals(InkOutcome.Written, session.finish(stroke(50f to 0f, 90f to 30f)))
        assertEquals(InkOutcome.Written, session.finish(stroke(400f to 0f, 440f to 30f)))
        val erased = session.finish(scribble(-10f, -5f, 120f, turns = 6, height = 45f))
        assertEquals(InkOutcome.Erased(2), erased)
        assertEquals(1, session.strokes.size)
        session.clear()
        assertEquals(InkOutcome.DeleteWord, session.finish(scribble(0f, 0f, 120f, turns = 6)))
    }

    @Test
    fun aTapEndsTheWritingWhetherOrNotThereIsInk() {
        val session = InkSession(unit = 2f)
        assertEquals(InkOutcome.Tapped, session.finish(stroke(5f to 5f, 6f to 5f)))
        session.finish(stroke(0f to 0f, 40f to 30f))
        // Far from the writing a touch is a tap, and the writing before it is still there for the recognizer.
        assertEquals(InkOutcome.Tapped, session.finish(stroke(900f to 500f, 901f to 500f)))
        assertEquals(1, session.strokes.size)
    }

    @Test
    fun aShortTouchBesideTheWritingIsADotAndPartOfIt() {
        val session = InkSession(unit = 2f)
        session.finish(stroke(100f to 40f, 100f to 120f))
        assertEquals(InkOutcome.Written, session.finish(stroke(100f to 5f, 101f to 5f)))
        assertEquals(2, session.strokes.size)
    }

    @Test
    fun theWrittenTextGoesAtTheCursorWithASpaceAfterAWord() {
        val field = TextFieldValue("hello", TextRange(5))
        assertEquals("hello world", TextEdits.insert(field, "world").text)
        assertEquals(TextRange(11), TextEdits.insert(field, "world").selection)
        assertEquals("hi", TextEdits.insert(TextFieldValue(""), "hi").text)
        // After a space there is no second one, and the rest of the text stays.
        assertEquals("a xb", TextEdits.insert(TextFieldValue("a b", TextRange(2)), "x").text)
        // A selection is replaced.
        assertEquals("h ello", TextEdits.insert(TextFieldValue("hxxxo", TextRange(1, 4)), "ell").text)
    }

    @Test
    fun scratchingOutDeletesTheWordBeforeTheCursor() {
        assertEquals("one two ", TextEdits.deleteWordBefore(TextFieldValue("one two three", TextRange(13))).text)
        assertEquals("one ", TextEdits.deleteWordBefore(TextFieldValue("one two", TextRange(7))).text)
        assertEquals("", TextEdits.deleteWordBefore(TextFieldValue("word", TextRange(4))).text)
        assertEquals("one two", TextEdits.deleteWordBefore(TextFieldValue("one two three ", TextRange(14))).text.trimEnd())
        assertEquals(TextRange(4), TextEdits.deleteWordBefore(TextFieldValue("one two", TextRange(7))).selection)
    }

    @Test
    fun theModelFollowsTheSystemLanguageThenItsBaseThenEnglish() {
        assertEquals(listOf("pt-PT", "pt", "en"), HandwritingLanguage.tags(Locale.forLanguageTag("pt-PT")))
        assertEquals(listOf("en-US", "en"), HandwritingLanguage.tags(Locale.forLanguageTag("en-US")))
        assertEquals(listOf("pt", "en"), HandwritingLanguage.tags(Locale.forLanguageTag("pt")))
    }

    private class Remembered(var on: Boolean = false) : HandwritingChoice {
        override fun saved() = on

        override fun save(on: Boolean) {
            this.on = on
        }
    }

    @Test
    fun theRecognizerIsOffUntilThePersonTurnsItOnAndWhileOffNothingIsCalled() {
        val handwriting = Handwriting(Remembered(), sdk = 32)
        assertFalse(handwriting.enabled)
        assertFalse(handwriting.ready)
        // Off, looking for a model and installing one do nothing: ML Kit is not called and nothing is downloaded.
        handwriting.refresh()
        handwriting.install()
        assertEquals(ModelState.Checking, handwriting.state)
        assertFalse(handwriting.platformHandles)
    }

    @Test
    fun theChoiceIsKeptAndTheRecognizerIsNotUsedWhereThePlatformWritesItself() {
        assertTrue(Handwriting(Remembered(), sdk = 34).platformHandles)
        assertTrue(Handwriting(Remembered(on = true), sdk = 32).enabled)
        // Turned on but with no model yet, the pen still behaves as a finger.
        assertFalse(Handwriting(Remembered(on = true), sdk = 32).ready)
    }
}
