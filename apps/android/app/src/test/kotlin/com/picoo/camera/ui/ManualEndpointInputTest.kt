package com.picoo.camera.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ManualEndpointInputTest {
    @Test
    fun pastedEndpointIsDistributedAcrossSegments() {
        val draft = ManualEndpointDraft.fromPastedText("192.168.1.108:4433")

        assertEquals(listOf("192", "168", "1", "108"), draft?.octets)
        assertEquals("4433", draft?.port)
        assertEquals(ManualEndpoint("192.168.1.108", 4433), draft?.validatedEndpoint())
        assertNull(ManualEndpointDraft.fromPastedText("192.168.1.108.7:4433"))
    }

    @Test
    fun defaultPortKeepsTypingFocusedOnTheAddress() {
        val draft = ManualEndpointDraft.from("")

        assertEquals(listOf("", "", "", ""), draft.octets)
        assertEquals("4433", draft.port)
    }

    @Test
    fun invalidOctetsAndPortsAreRejected() {
        assertNull(ManualEndpointDraft.from("192.168.1.999:4433").validatedEndpoint())
        assertNull(ManualEndpointDraft.from("192.168.1.108:0").validatedEndpoint())
        assertNull(ManualEndpointDraft.from("192.168.1.108:").validatedEndpoint())
        assertNull(ManualEndpointDraft.from("192.168..108:4433").validatedEndpoint())
    }

    @Test
    fun octetAdvancesWhenNoFurtherValidDigitCanBeAppended() {
        assertTrue(ManualEndpointDraft.shouldAdvanceOctet("192"))
        assertTrue(ManualEndpointDraft.shouldAdvanceOctet("26"))
    }
}
