package com.soyeht.next.android.ink

import android.os.Build
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.google.android.gms.tasks.Task
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
import java.util.Locale
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlinx.coroutines.CancellableContinuation
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine

/** Where the language model of the handwriting recognizer is. Nothing is downloaded before the person asks (Install). */
sealed interface ModelState {
    /** Not asked about yet. */
    data object Checking : ModelState

    data object NotInstalled : ModelState

    data object Downloading : ModelState

    data object Installed : ModelState

    /** The last install or removal failed; [kind] is the kind of failure, never its message. */
    data class Failed(val kind: String) : ModelState

    /** The device has no language model for this language. */
    data object NoModel : ModelState
}

/**
 * Handwriting on the message field, recognized on the device (ADR 0157). From Android 14 the platform handles a stylus in
 * a text field itself, through the keyboard, and this is not used. Below it, ML Kit's digital ink recognizer reads the
 * strokes with a language model the person installs from Settings: the model follows the system language, and is
 * downloaded once, when asked for, and not before.
 */
class Handwriting(private val locale: () -> Locale = { Locale.getDefault() }, sdk: Int = Build.VERSION.SDK_INT) {
    /** The platform writes in text fields itself: no ink area, no model. */
    val platformHandles = sdk >= PLATFORM_HANDWRITING_SDK

    var state: ModelState by mutableStateOf(ModelState.Checking)
        private set

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private val models = RemoteModelManager.getInstance()
    private var recognizer: DigitalInkRecognizer? = null
    private var model: DigitalInkRecognitionModel? = null

    /** Strokes are taken, and the model is ready: the composer lets the pen write. */
    val ready get() = !platformHandles && state == ModelState.Installed

    /** The model of the system language, or of its base language, or English: the first the recognizer has. */
    private fun modelOf(): DigitalInkRecognitionModel? {
        model?.let { return it }
        for (tag in HandwritingLanguage.tags(locale())) {
            val identifier = try {
                DigitalInkRecognitionModelIdentifier.fromLanguageTag(tag)
            } catch (failure: MlKitException) {
                // This tag has no model: the next one is tried.
                continue
            } ?: continue
            return DigitalInkRecognitionModel.builder(identifier).build().also { model = it }
        }
        return null
    }

    /** Asks the model manager whether the model is on the device. Nothing is downloaded. */
    fun refresh() {
        if (platformHandles) return
        val wanted = modelOf() ?: run {
            state = ModelState.NoModel
            return
        }
        scope.launch {
            state = try {
                if (models.isModelDownloaded(wanted).await()) ModelState.Installed else ModelState.NotInstalled
            } catch (failure: Exception) {
                failed("check", failure)
            }
        }
    }

    /** The person pressed Install: the model downloads, once. */
    fun install() {
        val wanted = modelOf() ?: return
        if (state == ModelState.Downloading) return
        state = ModelState.Downloading
        scope.launch {
            state = try {
                models.download(wanted, DownloadConditions.Builder().build()).await()
                ModelState.Installed
            } catch (failure: Exception) {
                failed("download", failure)
            }
        }
    }

    /** The person removed the model: the recognizer is closed and the files go. */
    fun remove() {
        val wanted = modelOf() ?: return
        recognizer?.close()
        recognizer = null
        scope.launch {
            state = try {
                models.deleteDownloadedModel(wanted).await()
                ModelState.NotInstalled
            } catch (failure: Exception) {
                failed("remove", failure)
            }
        }
    }

    /**
     * The text [strokes] write, or null when the recognizer read nothing or failed. [before] is the text before the
     * cursor, which helps the recognizer; the area is the size of the writing surface in pixels.
     */
    suspend fun recognize(strokes: List<InkStroke>, before: String, width: Float, height: Float): String? {
        if (!ready || strokes.isEmpty()) return null
        val wanted = modelOf() ?: return null
        val client = recognizer ?: DigitalInkRecognition.getClient(DigitalInkRecognizerOptions.builder(wanted).build()).also { recognizer = it }
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
        return try {
            client.recognize(ink, context).await().candidates.firstOrNull()?.text?.takeIf { it.isNotBlank() }
        } catch (failure: Exception) {
            failed("recognize", failure)
            null
        }
    }

    /** Ends the engine's work and closes the recognizer. */
    fun close() {
        recognizer?.close()
        scope.cancel()
    }

    /** Says which step failed and with what kind of exception, never its message (it can carry paths). */
    private fun failed(step: String, failure: Exception): ModelState {
        Log.w(TAG, "handwriting $step failed: ${failure.javaClass.simpleName}")
        return ModelState.Failed(step)
    }

    companion object {
        private const val TAG = "Handwriting"
        private const val PRE_CONTEXT = 20

        /** The first Android with the platform's own stylus handwriting in text fields. */
        const val PLATFORM_HANDWRITING_SDK = 34
    }
}

/** A Google task as a suspending call. */
private suspend fun <T> Task<T>.await(): T = suspendCancellableCoroutine { continuation: CancellableContinuation<T> ->
    addOnSuccessListener { continuation.resume(it) }
    addOnFailureListener { continuation.resumeWithException(it) }
}
