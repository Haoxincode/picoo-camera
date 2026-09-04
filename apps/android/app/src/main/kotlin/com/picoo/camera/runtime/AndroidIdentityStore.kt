package com.picoo.camera.runtime

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Stores the Ed25519 seed encrypted by a non-exportable Android Keystore key.
 * Authentication code never falls back to a plaintext identity file.
 */
internal class AndroidIdentityStore(context: Context) {
    private val preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)

    fun load(): ByteArray = synchronized(IDENTITY_LOCK) {
        val key = encryptionKey()
        val encoded = preferences.getString(ENCRYPTED_SECRET, null)
        if (encoded != null) return decrypt(key, Base64.decode(encoded, Base64.NO_WRAP))

        val secret = ByteArray(SECRET_BYTES).also(SecureRandom()::nextBytes)
        try {
            val sealed = encrypt(key, secret)
            check(
                preferences.edit()
                    .putString(ENCRYPTED_SECRET, Base64.encodeToString(sealed, Base64.NO_WRAP))
                    .commit(),
            ) { "could not persist encrypted sender identity" }
            return secret
        } catch (error: Throwable) {
            secret.fill(0)
            throw error
        }
    }

    private fun encryptionKey(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").run {
            init(
                KeyGenParameterSpec.Builder(
                    KEY_ALIAS,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                )
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .setRandomizedEncryptionRequired(true)
                    .build(),
            )
            generateKey()
        }
    }

    private fun encrypt(key: SecretKey, secret: ByteArray): ByteArray {
        val cipher = Cipher.getInstance(TRANSFORMATION).apply {
            init(Cipher.ENCRYPT_MODE, key)
            updateAAD(AAD)
        }
        return cipher.iv + cipher.doFinal(secret)
    }

    private fun decrypt(key: SecretKey, sealed: ByteArray): ByteArray {
        check(sealed.size > IV_BYTES) { "encrypted sender identity is truncated" }
        val cipher = Cipher.getInstance(TRANSFORMATION).apply {
            init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(TAG_BITS, sealed.copyOfRange(0, IV_BYTES)))
            updateAAD(AAD)
        }
        return cipher.doFinal(sealed.copyOfRange(IV_BYTES, sealed.size)).also {
            check(it.size == SECRET_BYTES) { "sender identity has invalid length" }
        }
    }

    private companion object {
        const val PREFERENCES = "picoo_secure_identity"
        const val ENCRYPTED_SECRET = "sender_ed25519"
        const val KEY_ALIAS = "picoo.camera.sender.identity.wrap"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val SECRET_BYTES = 32
        const val IV_BYTES = 12
        const val TAG_BITS = 128
        val AAD = "picoo-camera sender identity".toByteArray(Charsets.UTF_8)
        val IDENTITY_LOCK = Any()
    }
}
