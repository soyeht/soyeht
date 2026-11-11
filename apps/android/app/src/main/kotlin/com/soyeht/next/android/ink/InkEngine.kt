package com.soyeht.next.android.ink

/** A language model of the recognizer, by the language tag it was found for (`pt-BR`, `en`). */
data class InkModel(val tag: String)

/** Reads strokes as text. */
interface InkReader {
    /** The text [strokes] write, or null when nothing was read. [before] is the text before the cursor; the area is the writing surface in pixels. */
    suspend fun read(strokes: List<InkStroke>, before: String, width: Float, height: Float): String?

    fun close()
}

/**
 * Everything [Handwriting] asks of the recognizer library (Google's ML Kit, [MlKitInkEngine]), kept small so that a test
 * can count the calls. [Handwriting] calls [start] before anything else, and only once the person turned the recognizer
 * on; while it is off no method of this interface is called.
 */
interface InkEngine {
    /** Starts the library, which nothing in the app's manifest does. Called by each [Handwriting] before its first use, so it must do its work once per process and be harmless the second time. */
    fun start()

    /** The model for [tag], or null when the library has none for it. */
    fun modelFor(tag: String): InkModel?

    suspend fun isDownloaded(model: InkModel): Boolean

    suspend fun download(model: InkModel)

    suspend fun delete(model: InkModel)

    fun reader(model: InkModel): InkReader
}
