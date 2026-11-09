package com.soyeht.next.android.ink

import android.content.Context
import androidx.work.Configuration
import androidx.work.WorkManager
import com.google.android.gms.tasks.Task
import com.google.mlkit.common.MlKit
import com.google.mlkit.common.MlKitException
import com.google.mlkit.common.model.DownloadConditions
import com.google.mlkit.common.model.RemoteModelManager
import com.google.mlkit.vision.digitalink.DigitalInkRecognition
import com.google.mlkit.vision.digitalink.DigitalInkRecognitionModel
import com.google.mlkit.vision.digitalink.DigitalInkRecognitionModelIdentifier
import com.google.mlkit.vision.digitalink.DigitalInkRecognizer
import com.google.mlkit.vision.digitalink.DigitalInkRecognizerOptions
import com.google.mlkit.vision.digitalink.Ink
import com.google.mlkit.vision.digitalink.RecognitionContext
import com.google.mlkit.vision.digitalink.WritingArea
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlinx.coroutines.CancellableContinuation
import kotlinx.coroutines.suspendCancellableCoroutine

/**
 * ML Kit's Digital Ink Recognition behind [InkEngine]. The library's start-up provider and WorkManager's are removed
 * from the manifest (ADR 0157), so nothing of them runs until [start]; constructing this calls nothing.
 */
class MlKitInkEngine(private val context: Context) : InkEngine {
    private val models by lazy(LazyThreadSafetyMode.NONE) { RemoteModelManager.getInstance() }
    private val known = HashMap<String, DigitalInkRecognitionModel>()

    override fun start() {
        val application = context.applicationContext
        // ML Kit's model download runs on WorkManager, whose start-up entry the manifest removes.
        try {
            WorkManager.initialize(application, Configuration.Builder().build())
        } catch (initialized: IllegalStateException) {
            // Started already (by this app's earlier call): it is kept.
        }
        MlKit.initialize(application)
    }

    override fun modelFor(tag: String): InkModel? {
        val identifier = try {
            DigitalInkRecognitionModelIdentifier.fromLanguageTag(tag)
        } catch (failure: MlKitException) {
            // This tag has no model: the next one is tried.
            return null
        } ?: return null
        known.getOrPut(tag) { DigitalInkRecognitionModel.builder(identifier).build() }
        return InkModel(tag)
    }

    private fun native(model: InkModel) = checkNotNull(known[model.tag]) { "the model was not found by this engine" }

    override suspend fun isDownloaded(model: InkModel) = models.isModelDownloaded(native(model)).await()

    override suspend fun download(model: InkModel) {
        models.download(native(model), DownloadConditions.Builder().build()).await()
    }

    override suspend fun delete(model: InkModel) {
        models.deleteDownloadedModel(native(model)).await()
    }

    override fun reader(model: InkModel): InkReader = MlKitReader(
        DigitalInkRecognition.getClient(DigitalInkRecognizerOptions.builder(native(model)).build()),
    )
}

private class MlKitReader(private val recognizer: DigitalInkRecognizer) : InkReader {
    override suspend fun read(strokes: List<InkStroke>, before: String, width: Float, height: Float): String? {
        val ink = Ink.builder().apply {
            for (stroke in strokes) {
                addStroke(
                    Ink.Stroke.builder().apply {
                        for (point in stroke.points) addPoint(Ink.Point.create(point.x, point.y, point.time))
                    }.build(),
                )
            }
        }.build()
        val context = RecognitionContext.builder().setPreContext(before.takeLast(PRE_CONTEXT)).setWritingArea(WritingArea(width, height)).build()
        return recognizer.recognize(ink, context).await().candidates.firstOrNull()?.text?.takeIf { it.isNotBlank() }
    }

    override fun close() = recognizer.close()

    private companion object {
        const val PRE_CONTEXT = 20
    }
}

/** A Google task as a suspending call. */
private suspend fun <T> Task<T>.await(): T = suspendCancellableCoroutine { continuation: CancellableContinuation<T> ->
    addOnSuccessListener { continuation.resume(it) }
    addOnFailureListener { continuation.resumeWithException(it) }
}
