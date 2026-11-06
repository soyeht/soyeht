package com.soyeht.next.android

import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
import com.soyeht.next.android.scan.QrDecoder
import com.soyeht.next.android.scan.RepeatFilter
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The camera path up to the text: a frame in, what the code says out. The link itself is judged by the core. */
class QrDecoderTest {
    private val link = "soyeht-next://pair?engineId=6f1e2d3c-4b5a-4978-8a6b-5c4d3e2f1a0b&address=100.101.102.103:7443" +
        "&code=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA&engineKey=0EqyMnQrtKs6E2i9RhXk5tAiSrcaAWuvhSCjMsl3hzc"

    /** A frame: dark modules on a light ground, [scale] pixels to a module, padded to [stride] bytes a row. */
    private class Frame(val luma: ByteArray, val width: Int, val height: Int, val stride: Int)

    private fun frame(text: String, scale: Int = 4, stride: Int? = null, light: Boolean = true, turns: Int = 0): Frame {
        val matrix = QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, 0, 0, mapOf(EncodeHintType.MARGIN to 4))
        val side = matrix.width * scale
        val pixels = Array(side) { y -> IntArray(side) { x -> if (matrix.get(x / scale, y / scale)) 0 else 255 } }
        var turned = pixels
        repeat(turns) { turned = Array(side) { y -> IntArray(side) { x -> turned[side - 1 - x][y] } } }
        val row = stride ?: side
        val bytes = ByteArray(row * side) { 128.toByte() }
        for (y in 0 until side) {
            for (x in 0 until side) {
                val value = if (light) turned[y][x] else 255 - turned[y][x]
                bytes[y * row + x] = value.toByte()
            }
        }
        return Frame(bytes, side, side, row)
    }

    private fun read(frame: Frame) = QrDecoder().decode(frame.luma, frame.width, frame.height, frame.stride)

    @Test
    fun aPairingLinkIsReadFromTheFrame() {
        assertEquals(link, read(frame(link)))
    }

    @Test
    fun aRowThatIsLongerThanThePictureIsReadAsTheCameraGivesIt() {
        val padded = frame(link, stride = frame(link).width + 48)
        assertEquals(link, read(padded))
    }

    @Test
    fun aCodeIsReadWhicheverWayTheCameraIsTurned() {
        val read = (1..3).map { turns -> turns to read(frame(link, turns = turns)) }
        assertEquals(listOf(1 to link, 2 to link, 3 to link), read)
    }

    @Test
    fun aCodeInTheMiddleOfAWideFrameIsReadTurnedEitherWay() {
        val square = frame(link)
        for (turns in 0..3) {
            val code = frame(link, turns = turns)
            val width = code.width * 2
            val height = code.height + 40
            val canvas = ByteArray((width + 24) * height) { 120.toByte() }
            for (y in 0 until code.height) {
                for (x in 0 until code.width) {
                    canvas[(y + 20) * (width + 24) + x + code.width / 2] = code.luma[y * code.stride + x]
                }
            }
            assertEquals("turned $turns", link, QrDecoder().decode(canvas, width, height, width + 24))
        }
        assertEquals(square.width, square.height)
    }

    @Test
    fun aCodeShownLightOnDarkIsRead() {
        assertEquals(link, read(frame(link, light = false)))
    }

    @Test
    fun aCodeThatIsNotALinkIsStillReadSoTheCoreCanRefuseItInWords() {
        assertEquals("https://example.com", read(frame("https://example.com")))
    }

    @Test
    fun aFrameWithoutACodeReadsNothing() {
        val blank = Frame(ByteArray(64 * 64) { 200.toByte() }, 64, 64, 64)
        assertNull(read(blank))
    }

    @Test
    fun theSameCodeIsOfferedOnceUntilTheCameraHasLookedAwayLongEnough() {
        val filter = RepeatFilter(quietMillis = 1_000)
        assertTrue(filter.accept("a", 0))
        assertFalse(filter.accept("a", 500))
        assertTrue(filter.accept("b", 600))
        assertTrue(filter.accept("a", 700))
        assertTrue(filter.accept("a", 1_700))
    }
}
