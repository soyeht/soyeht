package com.soyeht.next.android

import java.io.File
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.KeyStoreException
import java.security.ProviderException
import javax.crypto.AEADBadTagException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

/** The credential's file logic without a phone: the Keystore is a [Sealer] that can be made to fail. */
class SealedFileTest {
    @get:Rule
    val folder = TemporaryFolder()

    /** Reverses the bytes, or throws what it is told to. */
    private class Fake : Sealer {
        var sealing: Exception? = null
        var opening: Exception? = null

        override fun seal(plain: ByteArray): ByteArray = sealing?.let { throw it } ?: plain.reversedArray()

        override fun open(sealed: ByteArray): ByteArray = opening?.let { throw it } ?: sealed.reversedArray()
    }

    private val logged = mutableListOf<String>()

    private fun file() = File(folder.root, "access.bin")

    private fun store(sealer: Sealer) = SealedFile(file(), sealer) { logged += it }

    @Test
    fun whatIsSavedIsLoadedAndNothingIsLeftBesideIt() {
        val sealer = Fake()
        store(sealer).save("key and token")
        assertEquals("key and token", store(sealer).load())
        assertEquals(listOf("access.bin"), folder.root.list()!!.toList())
        assertFalse(logged.joinToString().contains("token"))
    }

    @Test
    fun aSaveThatFailsLeavesTheEarlierAccessWhole() {
        val sealer = Fake()
        store(sealer).save("first")
        sealer.sealing = ProviderException("busy")
        try {
            store(sealer).save("second")
            fail("a failed seal was not reported")
        } catch (failure: IOException) {
            // The caller tells the person.
        }
        sealer.sealing = null
        assertEquals("first", store(sealer).load())
        assertEquals(listOf("access.bin"), folder.root.list()!!.toList())
    }

    @Test
    fun aSecondSaveReplacesTheFirst() {
        val sealer = Fake()
        store(sealer).save("first")
        store(sealer).save("second")
        assertEquals("second", store(sealer).load())
    }

    @Test
    fun nothingSavedIsNull() {
        assertNull(store(Fake()).load())
    }

    @Test
    fun aFileThatCanNeverBeReadAgainIsDroppedWithoutACrash() {
        val permanent = listOf<Exception>(AEADBadTagException("tag"), IllegalArgumentException("short"), IndexOutOfBoundsException())
        for (failure in permanent) {
            val sealer = Fake()
            store(sealer).save("secret")
            sealer.opening = failure
            assertNull(failure.javaClass.simpleName, store(sealer).load())
            assertFalse(failure.javaClass.simpleName, file().exists())
        }
        assertTrue(logged.all { it.startsWith("saved access cannot be read again: ") })
    }

    @Test
    fun aFileThatIsNotBase64IsDropped() {
        file().writeText("notbase64!!")
        assertNull(store(Fake()).load())
        assertFalse(file().exists())
    }

    @Test
    fun aFailureThatMayPassKeepsTheFile() {
        val transient = listOf<Exception>(KeyStoreException("busy"), ProviderException("restarting"), GeneralSecurityException(), IOException())
        for (failure in transient) {
            val sealer = Fake()
            store(sealer).save("secret")
            sealer.opening = failure
            assertNull(failure.javaClass.simpleName, store(sealer).load())
            assertTrue(failure.javaClass.simpleName, file().exists())
            sealer.opening = null
            assertEquals("secret", store(sealer).load())
        }
        assertTrue(logged.all { it.startsWith("saved access cannot be read now: ") })
    }

    @Test
    fun forgettingRemovesTheFile() {
        val sealer = Fake()
        store(sealer).save("secret")
        store(sealer).forget()
        assertNull(store(sealer).load())
        assertTrue(folder.root.list()!!.isEmpty())
    }
}
