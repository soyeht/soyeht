package com.soyeht.next.android.scan

import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.NotFoundException
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.ReaderException
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader

/**
 * Reads a QR code from the brightness plane of a camera frame. ZXing runs on the phone, with no
 * network and no Google service, and a code is read whatever way the camera is turned.
 *
 * What the code says is not judged here: the core's parser decides whether it is a pairing link.
 */
class QrDecoder {
    // A reader keeps the state of the frame it is reading: one decoder serves one thread.
    private val reader = QRCodeReader()
    private val hints = mapOf(DecodeHintType.TRY_HARDER to true)

    /**
     * @param luma the brightness plane, [rowStride] bytes per row, of which the first [width] are the picture.
     * @return the text of the code, or null when the frame holds none.
     */
    fun decode(luma: ByteArray, width: Int, height: Int, rowStride: Int = width): String? {
        read(luma, width, height, rowStride)?.let { return it }
        // A phone held upright sees a code upright on a screen turned a quarter: the detector copes
        // with some turns and not others, so the other two quarters are tried before giving up.
        val turned = rotated(luma, width, height, rowStride)
        read(turned, height, width, height)?.let { return it }
        return read(rotated(turned, height, width, height), width, height, width)
    }

    private fun read(luma: ByteArray, width: Int, height: Int, rowStride: Int): String? {
        val source = PlanarYUVLuminanceSource(luma, rowStride, height, 0, 0, width, height, false)
        // A code printed light on dark is read from the inverted picture.
        return read(source) ?: read(source.invert())
    }

    private fun read(source: com.google.zxing.LuminanceSource): String? = try {
        reader.decode(BinaryBitmap(HybridBinarizer(source)), hints).text
    } catch (_: NotFoundException) {
        null
    } catch (_: ReaderException) {
        // A finder pattern seen and a code not read (a checksum or a format error) is a frame without a code.
        null
    } finally {
        reader.reset()
    }
}

/** The brightness plane turned a quarter clockwise: [height] columns wide and [width] rows tall. */
internal fun rotated(luma: ByteArray, width: Int, height: Int, rowStride: Int): ByteArray {
    val turned = ByteArray(width * height)
    for (y in 0 until height) {
        for (x in 0 until width) {
            turned[x * height + (height - 1 - y)] = luma[y * rowStride + x]
        }
    }
    return turned
}

/** Offers each text once in a while: a camera sees the same code thirty times a second. */
class RepeatFilter(private val quietMillis: Long = QUIET_MILLIS) {
    private var last: String? = null
    private var at = 0L

    fun accept(text: String, nowMillis: Long): Boolean {
        if (text == last && nowMillis - at < quietMillis) return false
        last = text
        at = nowMillis
        return true
    }

    private companion object {
        const val QUIET_MILLIS = 3_000L
    }
}
