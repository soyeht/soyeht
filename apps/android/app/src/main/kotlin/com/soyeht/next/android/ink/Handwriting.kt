package com.soyeht.next.android.ink

import android.content.Context
import android.os.Build
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import java.util.Locale
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

/** What the recognizer was doing when it failed: [verb] finishes "Could not ... the model". */
enum class Step(val verb: String) {
    Check("check"),
    Download("download"),
    Remove("remove"),
}

/** Where the language model of the handwriting recognizer is. Nothing is downloaded before the person asks (Install). */
sealed interface ModelState {
    /** Not asked about yet. */
    data object Checking : ModelState

    data object NotInstalled : ModelState

    data object Downloading : ModelState

    data object Installed : ModelState

    /** The last [step] failed; what failed is logged by its kind, never its message. */
    data class Failed(val step: Step) : ModelState

    /** The device has no language model for this language. */
    data object NoModel : ModelState
}

/** Whether the person turned the on-device recognizer on: kept across launches, off until they choose. */
interface HandwritingChoice {
    fun saved(): Boolean

    fun save(on: Boolean)
}

/** The choice, in the app's private storage. */
class StoredHandwritingChoice(context: Context) : HandwritingChoice {
    private val preferences = context.getSharedPreferences("handwriting", Context.MODE_PRIVATE)

    override fun saved() = preferences.getBoolean(ON, false)

    override fun save(on: Boolean) {
        preferences.edit().putBoolean(ON, on).apply()
    }

    private companion object {
        const val ON = "on"
    }
}

/**
 * Handwriting on the message field, recognized on the device (ADR 0157). From Android 14 the platform handles a stylus in
 * a text field itself, through the keyboard, and this is not used. Below it, ML Kit's digital ink recognizer (behind
 * [engine]) reads the strokes with a language model the person installs from Settings: the model follows the system
 * language, and is downloaded once, when asked for, and not before.
 *
 * While the recognizer is off, nothing here calls [engine]: not at construction, not to look for a model, not to
 * install, recognize or remove one. [engine] is started when the person turns it on, or at launch if they had.
 */
class Handwriting(
    private val choice: HandwritingChoice,
    private val engine: InkEngine,
    private val locale: () -> Locale = { Locale.getDefault() },
    sdk: Int = Build.VERSION.SDK_INT,
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Main),
) {
    /** The platform writes in text fields itself: no ink area, no model. */
    val platformHandles = sdk >= PLATFORM_HANDWRITING_SDK

    var state: ModelState by mutableStateOf(ModelState.Checking)
        private set

    /**
     * The person turned the on-device recognizer on. It is off until they do: while it is off ML Kit is not called at
     * all (no check, no download, no recognizer), and a stylus touches the field as a finger does.
     */
    var enabled by mutableStateOf(choice.saved())
        private set

    // The model in use, once ML Kit was asked: the system language's, its base language's, or English (the first the recognizer has).
    private var model: InkModel? by mutableStateOf(null)
    private var reader: InkReader? = null
    private var started = false

    // The check, install or removal in flight: turning the recognizer off cancels it, so a late result sets nothing.
    private var job: Job? = null

    /** Strokes are taken, and the model is ready: the composer lets the pen write. */
    val ready get() = !platformHandles && enabled && state == ModelState.Installed

    /** The language of the model in use, or of the system when there is none, as the system names it. */
    val language: String get() = Locale.forLanguageTag(model?.tag ?: locale().toLanguageTag()).getDisplayLanguage(locale())

    private val active get() = !platformHandles && enabled

    /** The person turned the recognizer on or off in Settings. Turning it on only looks for a model; Install downloads one. */
    fun enable(on: Boolean) {
        if (on == enabled) return
        enabled = on
        choice.save(on)
        if (on) {
            state = ModelState.Checking
            refresh()
        } else {
            job?.cancel()
            job = null
            reader?.close()
            reader = null
            state = ModelState.Checking
        }
    }

    private fun live(): InkEngine {
        if (!started) {
            engine.start()
            started = true
        }
        return engine
    }

    private fun resolve(): InkModel? {
        model?.let { return it }
        val library = live()
        return HandwritingLanguage.tags(locale()).firstNotNullOfOrNull { library.modelFor(it) }?.also { model = it }
    }

    /** Runs [found] with the model in use; with none for this language the state says so, and when ML Kit cannot start it says [step] failed. */
    private inline fun withModel(step: Step, found: (InkModel) -> Unit) {
        val wanted = try {
            resolve()
        } catch (failure: Exception) {
            state = failed(step, failure)
            return
        }
        if (wanted == null) state = ModelState.NoModel else found(wanted)
    }

    private fun launchStep(step: Step, work: suspend (InkEngine) -> ModelState) {
        job?.cancel()
        job = scope.launch {
            val result = try {
                work(engine)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                failed(step, failure)
            }
            state = result
        }
    }

    /** Asks the model manager whether the model is on the device. Nothing is downloaded. */
    fun refresh() {
        if (!active || job?.isActive == true) return
        withModel(Step.Check) { wanted ->
            launchStep(Step.Check) { if (it.isDownloaded(wanted)) ModelState.Installed else ModelState.NotInstalled }
        }
    }

    /** The person pressed Install: the model downloads, once. */
    fun install() {
        if (!active || state == ModelState.Downloading) return
        withModel(Step.Download) { wanted ->
            state = ModelState.Downloading
            launchStep(Step.Download) {
                it.download(wanted)
                ModelState.Installed
            }
        }
    }

    /** The person removed the model: the recognizer is closed and the files go. */
    fun remove() {
        if (!active) return
        withModel(Step.Remove) { wanted ->
            reader?.close()
            reader = null
            launchStep(Step.Remove) {
                it.delete(wanted)
                ModelState.NotInstalled
            }
        }
    }

    /** The person pressed Retry after a failure: the step that failed is tried again, and no other. */
    fun retry() {
        val failure = state as? ModelState.Failed ?: return
        when (failure.step) {
            Step.Check -> {
                state = ModelState.Checking
                refresh()
            }
            Step.Download -> install()
            Step.Remove -> remove()
        }
    }

    /**
     * The text [strokes] write, or null when the recognizer read nothing or failed. [before] is the text before the
     * cursor, which helps the recognizer; the area is the size of the writing surface in pixels.
     */
    suspend fun recognize(strokes: List<InkStroke>, before: String, width: Float, height: Float): String? {
        if (!ready || strokes.isEmpty()) return null
        val wanted = model ?: return null
        return try {
            val current = reader ?: live().reader(wanted).also { reader = it }
            current.read(strokes, before, width, height)
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: Exception) {
            log("recognize", failure)
            null
        }
    }

    /** Ends the engine's work and closes the recognizer. */
    fun close() {
        reader?.close()
        reader = null
        scope.cancel()
    }

    private fun failed(step: Step, failure: Exception): ModelState {
        log(step.verb, failure)
        return ModelState.Failed(step)
    }

    /** Logs which step failed and with what kind of exception, never its message (it can carry paths). */
    private fun log(what: String, failure: Exception) {
        Log.w(TAG, "handwriting $what failed: ${failure.javaClass.simpleName}")
    }

    companion object {
        private const val TAG = "Handwriting"

        /** The first Android with the platform's own stylus handwriting in text fields. */
        const val PLATFORM_HANDWRITING_SDK = 34
    }
}
