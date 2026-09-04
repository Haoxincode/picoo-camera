package com.picoo.camera.runtime

import android.content.Context
import android.util.Base64
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import java.security.KeyStore
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class AndroidIdentityStoreTest {
    private lateinit var context: Context

    @Before
    fun setUp() {
        context = ApplicationProvider.getApplicationContext()
        clearTestIdentity()
    }

    @After
    fun tearDown() {
        clearTestIdentity()
    }

    @Test
    fun encryptedIdentityPersistsAndCorruptionFailsClosed() {
        val first = AndroidIdentityStore(context).load()
        val second = AndroidIdentityStore(context).load()
        assertEquals(SECRET_BYTES, first.size)
        assertArrayEquals(first, second)

        val stored = preferences().getString(ENCRYPTED_SECRET, null)
        assertFalse(stored.isNullOrBlank())
        assertFalse(
            "the Ed25519 seed must not be persisted as plaintext",
            Base64.encodeToString(first, Base64.NO_WRAP) == stored,
        )

        val corrupt = byteArrayOf(1, 2, 3, 4)
        assertTrue(
            preferences().edit()
                .putString(ENCRYPTED_SECRET, Base64.encodeToString(corrupt, Base64.NO_WRAP))
                .commit(),
        )
        assertTrue(runCatching { AndroidIdentityStore(context).load() }.isFailure)
        assertArrayEquals(
            corrupt,
            Base64.decode(preferences().getString(ENCRYPTED_SECRET, null), Base64.NO_WRAP),
        )

        first.fill(0)
        second.fill(0)
    }

    private fun clearTestIdentity() {
        preferences().edit().clear().commit()
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        if (keyStore.containsAlias(KEY_ALIAS)) keyStore.deleteEntry(KEY_ALIAS)
    }

    private fun preferences() = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)

    private companion object {
        const val PREFERENCES = "picoo_secure_identity"
        const val ENCRYPTED_SECRET = "sender_ed25519"
        const val KEY_ALIAS = "picoo.camera.sender.identity.wrap"
        const val SECRET_BYTES = 32
    }
}
