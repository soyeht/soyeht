package com.soyeht.next.android

import com.soyeht.next.android.ink.Handwriting
import com.soyeht.next.android.ink.HandwritingChoice
import com.soyeht.next.android.ink.InkEngine
import com.soyeht.next.android.ink.InkModel
import com.soyeht.next.android.ink.InkPoint
import com.soyeht.next.android.ink.InkReader
import com.soyeht.next.android.ink.InkStroke
import com.soyeht.next.android.ink.ModelState
import com.soyeht.next.android.ink.Once
import com.soyeht.next.android.ink.Step
import java.util.Locale
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class HandwritingTest {
    private class Remembered(var on: Boolean = false) : HandwritingChoice {
        val saves = mutableListOf<Boolean>()

        override fun saved() = on

        override fun save(on: Boolean) {
            this.on = on
            saves += on
        }
    }

    private class FakeReader(val log: MutableList<String>) : InkReader {
        var closed = false

        override suspend fun read(strokes: List<InkStroke>, before: String, width: Float, height: Float): String? {
            log += "read"
            return "word"
        }

        override fun close() {
            closed = true
            log += "close"
        }
    }

    /** Counts every call: while the recognizer is off the list must stay empty. */
    private class FakeEngine(var models: Set<String> = setOf("en", "pt"), private val onceInProcess: Boolean = false) : InkEngine {
        val calls = mutableListOf<String>()
        var downloaded = false
        var failDelete = false
        var failCheck = false
        var failDownload = false
        private var initialized = false
        private val once = Once()
        var gate: CompletableDeferred<Unit>? = null
        val readers = mutableListOf<FakeReader>()

        // Like the library's own initialize: it throws when called a second time in the same process.
        override fun start() {
            val work = {
                calls += "start"
                check(!initialized) { "already initialized" }
                initialized = true
            }
            if (onceInProcess) once.run(work) else work()
        }

        override fun modelFor(tag: String): InkModel? {
            calls += "modelFor:$tag"
            return if (tag in models) InkModel(tag) else null
        }

        override suspend fun isDownloaded(model: InkModel): Boolean {
            calls += "isDownloaded"
            if (failCheck) throw IllegalStateException("no")
            gate?.await()
            return downloaded
        }

        override suspend fun download(model: InkModel) {
            calls += "download"
            if (failDownload) throw IllegalStateException("no")
            gate?.await()
            downloaded = true
        }

        override suspend fun delete(model: InkModel) {
            calls += "delete"
            if (failDelete) throw IllegalStateException("no")
            downloaded = false
        }

        override fun reader(model: InkModel): InkReader {
            calls += "reader"
            return FakeReader(calls).also { readers += it }
        }
    }

    private val portuguese = { Locale.forLanguageTag("pt-BR") }
    private val strokes = listOf(InkStroke(listOf(InkPoint(0f, 0f, 0L), InkPoint(5f, 5f, 10L))))

    private fun handwriting(choice: Remembered, engine: FakeEngine, sdk: Int = 32) =
        Handwriting(choice, engine, portuguese, sdk, CoroutineScope(Dispatchers.Unconfined))

    @Test
    fun whileOffNothingIsCalledNotAtConstructionNorForAnyEntryPoint() = runBlocking {
        val engine = FakeEngine()
        val choice = Remembered()
        val handwriting = handwriting(choice, engine)
        assertFalse(handwriting.enabled)
        assertFalse(handwriting.ready)
        handwriting.refresh()
        handwriting.install()
        handwriting.remove()
        handwriting.retry()
        assertNull(handwriting.recognize(strokes, "", 100f, 100f))
        handwriting.enable(false)
        handwriting.close()
        assertEquals(emptyList<String>(), engine.calls)
        assertEquals(emptyList<Boolean>(), choice.saves)
        assertEquals(ModelState.Checking, handwriting.state)
        // Where the platform writes itself the recognizer is never used, even if the choice was kept.
        val platform = handwriting(Remembered(on = true), engine, sdk = 34)
        platform.refresh()
        platform.install()
        platform.remove()
        assertEquals(emptyList<String>(), engine.calls)
        assertFalse(platform.ready)
    }

    @Test
    fun turningItOnSavesTheChoiceThenStartsTheLibraryThenLooksForTheModel() {
        val engine = FakeEngine(models = setOf("pt"))
        val choice = Remembered()
        val handwriting = handwriting(choice, engine)
        handwriting.enable(true)
        assertEquals(listOf(true), choice.saves)
        assertEquals(listOf("start", "modelFor:pt-BR", "modelFor:pt", "isDownloaded"), engine.calls)
        assertEquals(ModelState.NotInstalled, handwriting.state)
        // Nothing is downloaded until Install.
        assertFalse(engine.calls.contains("download"))
        handwriting.install()
        assertEquals(ModelState.Installed, handwriting.state)
        assertEquals(listOf("start", "modelFor:pt-BR", "modelFor:pt", "isDownloaded", "download"), engine.calls)
        // The library is started once.
        handwriting.refresh()
        assertEquals(1, engine.calls.count { it == "start" })
    }

    @Test
    fun aChoiceKeptFromBeforeStartsTheLibraryOnlyWhenTheAppAsksForTheModel() {
        val engine = FakeEngine()
        val handwriting = handwriting(Remembered(on = true), engine)
        assertEquals(emptyList<String>(), engine.calls)
        handwriting.refresh()
        assertEquals("start", engine.calls.first())
    }

    @Test
    fun turningItOffSavesTheChoiceClosesTheReaderAndCancelsAnInstallInFlight() = runBlocking {
        val engine = FakeEngine().apply { downloaded = true }
        val choice = Remembered()
        val handwriting = handwriting(choice, engine)
        handwriting.enable(true)
        assertTrue(handwriting.ready)
        assertEquals("word", handwriting.recognize(strokes, "a", 100f, 100f))
        val reader = engine.readers.single()
        handwriting.enable(false)
        assertEquals(listOf(true, false), choice.saves)
        assertTrue(reader.closed)
        assertFalse(handwriting.ready)

        // A download in flight when the person turns it off sets nothing when it ends.
        val second = FakeEngine()
        val other = handwriting(Remembered(), second)
        other.enable(true)
        assertEquals(ModelState.NotInstalled, other.state)
        second.gate = CompletableDeferred()
        other.install()
        assertEquals(ModelState.Downloading, other.state)
        other.enable(false)
        second.gate?.complete(Unit)
        assertFalse(second.downloaded)
        assertEquals(ModelState.Checking, other.state)
        // A check in flight is cancelled the same way.
        val third = FakeEngine().apply { gate = CompletableDeferred(); downloaded = true }
        val last = handwriting(Remembered(), third)
        last.enable(true)
        last.enable(false)
        third.gate?.complete(Unit)
        assertEquals(ModelState.Checking, last.state)
    }

    @Test
    fun aFailedRemovalOffersRetryThatRemovesAndNeverInstalls() {
        val engine = FakeEngine().apply { downloaded = true; failDelete = true }
        val handwriting = handwriting(Remembered(), engine)
        handwriting.enable(true)
        handwriting.remove()
        assertEquals(ModelState.Failed(Step.Remove), handwriting.state)
        engine.failDelete = false
        engine.calls.clear()
        handwriting.retry()
        assertEquals(listOf("delete"), engine.calls)
        assertEquals(ModelState.NotInstalled, handwriting.state)
    }

    @Test
    fun theLanguageIsTheOneOfTheModelInUseNotTheSystemOne() {
        val engine = FakeEngine(models = setOf("en"))
        val handwriting = handwriting(Remembered(), engine)
        handwriting.enable(true)
        val english = Locale.forLanguageTag("en").getDisplayLanguage(portuguese())
        assertEquals(english, handwriting.language)
        assertNotEquals(Locale.forLanguageTag("pt").getDisplayLanguage(portuguese()), handwriting.language)
        // With no model at all it names the system language.
        val none = handwriting(Remembered(), FakeEngine(models = emptySet()))
        none.enable(true)
        assertEquals(ModelState.NoModel, none.state)
        assertEquals(Locale.forLanguageTag("pt").getDisplayLanguage(portuguese()), none.language)
    }

    @Test
    fun aFailedCheckOffersRetryThatChecksAndAFailedDownloadOffersRetryThatDownloads() {
        val engine = FakeEngine().apply { failCheck = true }
        val handwriting = handwriting(Remembered(), engine)
        handwriting.enable(true)
        assertEquals(ModelState.Failed(Step.Check), handwriting.state)
        engine.failCheck = false
        engine.calls.clear()
        handwriting.retry()
        assertEquals(listOf("isDownloaded"), engine.calls)
        assertEquals(ModelState.NotInstalled, handwriting.state)

        engine.failDownload = true
        handwriting.install()
        assertEquals(ModelState.Failed(Step.Download), handwriting.state)
        engine.failDownload = false
        engine.calls.clear()
        handwriting.retry()
        assertEquals(listOf("download"), engine.calls)
        assertEquals(ModelState.Installed, handwriting.state)
        // With nothing failed Retry does nothing.
        engine.calls.clear()
        handwriting.retry()
        assertEquals(emptyList<String>(), engine.calls)
    }

    @Test
    fun aSecondHandwritingOverTheSameEngineStartsAgainOnlyWhereTheEngineStartsOncePerProcess() {
        // The activity is backed out of and opened again while the process lives: the model is built again.
        val bare = FakeEngine()
        handwriting(Remembered(), bare).enable(true)
        val second = handwriting(Remembered(), bare)
        second.enable(true)
        // An engine that starts the library on each call fails the second one, and Retry cannot mend it.
        assertEquals(ModelState.Failed(Step.Check), second.state)

        val shared = FakeEngine(onceInProcess = true)
        val first = handwriting(Remembered(), shared)
        first.enable(true)
        first.close()
        val again = handwriting(Remembered(on = true), shared)
        again.refresh()
        assertEquals(ModelState.NotInstalled, again.state)
        assertEquals(1, shared.calls.count { it == "start" })
        again.install()
        assertEquals(ModelState.Installed, again.state)
    }

    @Test
    fun onceRunsAnActionOnceAndRunsItAgainOnlyIfItFailed() {
        val once = Once()
        var runs = 0
        once.run { runs += 1 }
        once.run { runs += 1 }
        assertEquals(1, runs)
        val retried = Once()
        var attempts = 0
        try {
            retried.run { attempts += 1; throw IllegalStateException("no") }
        } catch (expected: IllegalStateException) {
            // The failure reaches the caller, and the action did not count as run.
        }
        retried.run { attempts += 1 }
        retried.run { attempts += 1 }
        assertEquals(2, attempts)
    }
}
