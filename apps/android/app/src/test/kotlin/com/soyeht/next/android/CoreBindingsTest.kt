package com.soyeht.next.android

import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import uniffi.soyeht_android.Core
import uniffi.soyeht_android.CoreException
import uniffi.soyeht_android.Link
import uniffi.soyeht_android.Listener

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
        assertNull(core.composer("pane"))
        assertNull(core.harnessChoices("pane"))
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
}
