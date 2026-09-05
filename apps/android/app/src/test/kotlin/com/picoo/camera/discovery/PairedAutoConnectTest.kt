package com.picoo.camera.discovery

import com.picoo.camera.jni.PicooNative
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PairedAutoConnectTest {
    private fun receiver(id: String, name: String = id) =
        PicooNative.DiscoveredReceiver(
            receiverId = id,
            displayName = name,
            host = "192.168.1.10",
            quicPort = 4433,
            platform = "windows",
        )

    @Test
    fun picksFirstPairedNotYetAttempted() {
        val pick = PairedAutoConnect.pick(
            discovered = listOf(receiver("a"), receiver("b"), receiver("c")),
            pairedReceiverIds = setOf("b", "c"),
            sessionBusy = false,
            alreadyAttemptedIds = emptySet(),
        )
        assertEquals("b", pick?.receiverId)
    }

    @Test
    fun skipsAlreadyAttempted() {
        val pick = PairedAutoConnect.pick(
            discovered = listOf(receiver("b"), receiver("c")),
            pairedReceiverIds = setOf("b", "c"),
            sessionBusy = false,
            alreadyAttemptedIds = setOf("b"),
        )
        assertEquals("c", pick?.receiverId)
    }

    @Test
    fun returnsNullWhenSessionBusy() {
        val pick = PairedAutoConnect.pick(
            discovered = listOf(receiver("b")),
            pairedReceiverIds = setOf("b"),
            sessionBusy = true,
            alreadyAttemptedIds = emptySet(),
        )
        assertNull(pick)
    }

    @Test
    fun returnsNullWhenNoPairedMatch() {
        val pick = PairedAutoConnect.pick(
            discovered = listOf(receiver("x")),
            pairedReceiverIds = setOf("b"),
            sessionBusy = false,
            alreadyAttemptedIds = emptySet(),
        )
        assertNull(pick)
    }

    @Test
    fun remoteStopFromLiveSessionSuppressesNewAutoConnect() {
        for (liveStatus in listOf(
            PicooNative.STATUS_STREAMING,
            PicooNative.STATUS_NETWORK_UNSTABLE,
            PicooNative.STATUS_PERMISSION_REQUIRED,
        )) {
            assertTrue(
                PairedAutoConnect.shouldSuppressAfterRemoteStop(
                    previousStatus = liveStatus,
                    currentStatus = PicooNative.STATUS_DISCONNECTED,
                ),
            )
        }
    }

    @Test
    fun reconnectingAndNonDisconnectedTransitionsRemainAutomatic() {
        assertFalse(
            PairedAutoConnect.shouldSuppressAfterRemoteStop(
                previousStatus = PicooNative.STATUS_RECONNECTING,
                currentStatus = PicooNative.STATUS_DISCONNECTED,
            ),
        )
        assertFalse(
            PairedAutoConnect.shouldSuppressAfterRemoteStop(
                previousStatus = PicooNative.STATUS_STREAMING,
                currentStatus = PicooNative.STATUS_RECONNECTING,
            ),
        )
    }
}
