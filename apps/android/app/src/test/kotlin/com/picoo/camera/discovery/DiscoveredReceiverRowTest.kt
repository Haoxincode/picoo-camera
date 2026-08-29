package com.picoo.camera.discovery

import com.picoo.camera.jni.PicooNative
import org.junit.Assert.assertEquals
import org.junit.Test

class DiscoveredReceiverRowTest {
    private fun receiver(
        pairingState: String? = "open",
        name: String = "Work PC",
    ) = PicooNative.DiscoveredReceiver(
        receiverId = "recv-1",
        displayName = name,
        host = "192.168.1.20",
        quicPort = 4433,
        pairingState = pairingState,
    )

    @Test
    fun openAndUntrustedIsReady() {
        assertEquals("Ready", DiscoveredReceiverRow.readinessLabel("open", false))
        assertEquals(
            "Work PC · Windows · Ready",
            DiscoveredReceiverRow.format(receiver("open"), locallyTrusted = false),
        )
    }

    @Test
    fun locallyTrustedIsPairedEvenIfTxtOpen() {
        assertEquals("Paired", DiscoveredReceiverRow.readinessLabel("open", true))
        assertEquals(
            "Work PC · Windows · Paired",
            DiscoveredReceiverRow.format(receiver("open"), locallyTrusted = true),
        )
    }

    @Test
    fun pairedOnlyTxtIsPaired() {
        assertEquals("Paired", DiscoveredReceiverRow.readinessLabel("paired_only", false))
    }

    @Test
    fun nullPairingStateDefaultsToReady() {
        assertEquals("Ready", DiscoveredReceiverRow.readinessLabel(null, false))
    }
}
