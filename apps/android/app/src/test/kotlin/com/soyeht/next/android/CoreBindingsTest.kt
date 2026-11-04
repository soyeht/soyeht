package com.soyeht.next.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import uniffi.soyeht_android.Core
import uniffi.soyeht_android.CoreException
import uniffi.soyeht_android.Link
import uniffi.soyeht_android.Listener
import uniffi.soyeht_android.MarkdownBlock
import uniffi.soyeht_android.codeCopy
import uniffi.soyeht_android.markdownBlocks

/** The generated bindings reach the core library: the rules themselves are tested in Rust. */
class CoreBindingsTest {
    private class Silent : Listener {
        override fun changed() = Unit

        override fun fingerprint(fingerprint: String) = Unit

        override fun waitingForApproval() = Unit
    }

    private fun core(): Core {
        assumeTrue("SOYEHT_CORE_LIB_DIR names the directory of the core library", System.getenv("SOYEHT_CORE_LIB_DIR") != null)
        return Core(Silent())
    }

    @Test
    fun aNewCoreIsConnectingAndShowsNothing() {
        val core = core()
        assertTrue(core.overview().link is Link.Connecting)
        assertTrue(core.overview().workspaces.isEmpty())
        assertNull(core.terminal("pane"))
        assertNull(core.conversation("pane"))
    }

    @Test
    fun aTextThatIsNotAPairingLinkIsRefusedInWords() {
        val failure = runCatching { core().pair("https://example.com", "Phone") }.exceptionOrNull()
        assertTrue(failure is CoreException.Failed)
        assertTrue((failure as CoreException.Failed).reason.contains("pairing link"))
    }

    @Test
    fun accessThatIsNotAccessAsksToPairAgain() {
        val failure = runCatching { core().connect("not json") }.exceptionOrNull()
        assertTrue(failure is CoreException.PairAgain)
    }

    @Test
    fun markdownBlocksCrossTheBoundaryNestedAsTheVectorsHaveThem() {
        core()
        val blocks = markdownBlocks("1. one **bold**\n2. two\n   - inner\n\n> ```sh\n> ls -a\n> ```\n\n| a | b |\n|---|---|\n| 1 | 2 |", streaming = false)
        val list = blocks[0] as MarkdownBlock.ItemList
        assertEquals(1UL, list.start)
        assertEquals(2, list.items.size)
        assertTrue(list.items[0].blocks.single().let { it is MarkdownBlock.Paragraph && it.runs.any { run -> run.bold } })
        assertTrue(list.items[1].blocks.any { it is MarkdownBlock.ItemList })
        val quote = blocks[1] as MarkdownBlock.Quote
        val code = quote.blocks.single() as MarkdownBlock.Code
        assertEquals("ls -a", code.text)
        assertEquals("sh", code.language)
        val table = blocks[2] as MarkdownBlock.Table
        assertEquals(listOf("a", "b"), table.header.cells.map { it.runs.single().text })
    }

    @Test
    fun theCopyButtonSaysWhatTheContractSays() {
        core()
        val copy = codeCopy()
        assertEquals("Copy code", copy.label)
        assertEquals("Copied", copy.copied)
        assertEquals(1500UL, copy.feedbackMillis)
    }
}
