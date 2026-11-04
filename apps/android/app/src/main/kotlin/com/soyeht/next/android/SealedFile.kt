package com.soyeht.next.android

import android.security.keystore.KeyPermanentlyInvalidatedException
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.GeneralSecurityException
import java.security.UnrecoverableKeyException
import java.util.Base64
import javax.crypto.AEADBadTagException

/** Seals bytes so that only this phone can open them again. */
interface Sealer {
    @Throws(GeneralSecurityException::class)
    fun seal(plain: ByteArray): ByteArray

    @Throws(GeneralSecurityException::class)
    fun open(sealed: ByteArray): ByteArray
}

/**
 * One sealed string kept in a file. It is written whole or not at all, and read back without ever
 * crashing the app. The Android-free part of [CredentialStore], so a unit test can run it.
 */
class SealedFile(
    private val file: File,
    private val sealer: Sealer,
    private val log: (String) -> Unit = {},
) {
    private val partial = File(file.path + ".new")

    /**
     * The string, or null: there is none, or it cannot be read now. A file that can never be read again
     * (the tag does not match, the key is gone or was invalidated, the bytes are not a sealed string) is
     * deleted, so the person pairs again; a failure that may pass (the Keystore is busy, the storage
     * cannot be read at this moment) keeps the file and asks again next time. Only the kind of a failure
     * is logged.
     */
    fun load(): String? {
        if (!file.exists()) return null
        return try {
            String(sealer.open(Base64.getDecoder().decode(file.readText(Charsets.US_ASCII).trim())), Charsets.UTF_8)
        } catch (failure: AEADBadTagException) {
            lost(failure)
        } catch (failure: KeyPermanentlyInvalidatedException) {
            lost(failure)
        } catch (failure: UnrecoverableKeyException) {
            lost(failure)
        } catch (failure: IllegalArgumentException) {
            // Not base64, or too short to hold what it seals.
            lost(failure)
        } catch (failure: IndexOutOfBoundsException) {
            lost(failure)
        } catch (failure: GeneralSecurityException) {
            unreadable(failure)
        } catch (failure: IOException) {
            unreadable(failure)
        } catch (failure: RuntimeException) {
            // The Keystore reports a provider that is busy or restarting this way.
            unreadable(failure)
        }
    }

    /** Writes the string whole or not at all: a file that was cut short would be a lost credential. */
    @Throws(IOException::class)
    fun save(text: String) {
        val sealed = try {
            sealer.seal(text.toByteArray(Charsets.UTF_8))
        } catch (failure: GeneralSecurityException) {
            throw IOException("The text could not be sealed.", failure)
        } catch (failure: RuntimeException) {
            throw IOException("The text could not be sealed.", failure)
        }
        FileOutputStream(partial).use { out ->
            out.write(Base64.getEncoder().encode(sealed))
            out.fd.sync()
        }
        Files.move(partial.toPath(), file.toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
    }

    fun forget() {
        file.delete()
        partial.delete()
    }

    private fun lost(failure: Exception): String? {
        log("saved access cannot be read again: ${failure.javaClass.simpleName}")
        forget()
        return null
    }

    private fun unreadable(failure: Exception): String? {
        log("saved access cannot be read now: ${failure.javaClass.simpleName}")
        return null
    }
}
