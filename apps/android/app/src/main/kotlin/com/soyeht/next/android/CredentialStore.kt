package com.soyeht.next.android

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import android.util.Base64
import android.util.Log
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.UnrecoverableKeyException
import javax.crypto.AEADBadTagException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The access this phone was given by the computer: its key and token. It is encrypted with an Android
 * Keystore key that never leaves the device, kept in the app's private storage (no backup, no
 * transfer to another phone), and never logged or put in an intent.
 */
class CredentialStore(context: Context) {
    private val file = File(context.filesDir, "access.bin")
    private val partial = File(context.filesDir, "access.bin.new")

    /**
     * The saved access, or null: there is none, or it cannot be read now. A file that can never be read
     * again (the tag does not match, the key is gone or was invalidated, the bytes are not a sealed
     * credential) is deleted, so the person pairs again; a failure that may pass (the Keystore is busy,
     * the storage cannot be read at this moment) keeps the file and asks again next time. Nothing here
     * crashes the app, and only the kind of a failure is logged.
     */
    fun load(): String? {
        if (!file.exists()) return null
        return try {
            val bytes = Base64.decode(file.readText(), Base64.NO_WRAP)
            val cipher = Cipher.getInstance(TRANSFORM)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(TAG_BITS, bytes.copyOfRange(0, IV_BYTES)))
            String(cipher.doFinal(bytes, IV_BYTES, bytes.size - IV_BYTES), Charsets.UTF_8)
        } catch (failure: AEADBadTagException) {
            lost(failure)
        } catch (failure: KeyPermanentlyInvalidatedException) {
            lost(failure)
        } catch (failure: UnrecoverableKeyException) {
            lost(failure)
        } catch (failure: IllegalArgumentException) {
            // Not base64, or too short to hold an initialization vector.
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

    /** Writes the access whole or not at all: a file that was cut short would be a lost credential. */
    @Throws(IOException::class)
    fun save(credential: String) {
        val sealed = try {
            val cipher = Cipher.getInstance(TRANSFORM)
            cipher.init(Cipher.ENCRYPT_MODE, key())
            cipher.iv + cipher.doFinal(credential.toByteArray(Charsets.UTF_8))
        } catch (failure: GeneralSecurityException) {
            throw IOException("The credential could not be sealed.", failure)
        } catch (failure: RuntimeException) {
            throw IOException("The credential could not be sealed.", failure)
        }
        FileOutputStream(partial).use { out ->
            out.write(Base64.encodeToString(sealed, Base64.NO_WRAP).toByteArray(Charsets.US_ASCII))
            out.fd.sync()
        }
        Files.move(partial.toPath(), file.toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
    }

    fun forget() {
        file.delete()
        partial.delete()
    }

    private fun lost(failure: Exception): String? {
        Log.w(TAG, "saved access cannot be read again: ${failure.javaClass.simpleName}")
        forget()
        return null
    }

    private fun unreadable(failure: Exception): String? {
        Log.w(TAG, "saved access cannot be read now: ${failure.javaClass.simpleName}")
        return null
    }

    private fun key(): SecretKey {
        val store = KeyStore.getInstance(PROVIDER).apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, PROVIDER)
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(KEY_BITS)
                .build(),
        )
        return generator.generateKey()
    }

    private companion object {
        const val TAG = "soyeht"
        const val PROVIDER = "AndroidKeyStore"
        const val ALIAS = "soyeht-next-access"
        const val TRANSFORM = "AES/GCM/NoPadding"
        const val KEY_BITS = 256
        const val TAG_BITS = 128
        const val IV_BYTES = 12
    }
}
