package com.picoo.camera.discovery

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertNull
import org.junit.Test

class DiscoveryTxtTest {
    @Test
    fun copiesIteratorOnlyPlatformMapWithoutCallingToArray() {
        val raw = IteratorOnlyMap(
            linkedMapOf(
                "id" to "receiver-1".encodeToByteArray(),
                "name" to "Office PC".encodeToByteArray(),
            ),
        )

        val attributes = DiscoveryTxt.copyForJni(raw)

        assertArrayEquals(arrayOf("id", "name"), attributes?.keys)
        assertArrayEquals("receiver-1".encodeToByteArray(), attributes?.values?.get(0))
        assertArrayEquals("Office PC".encodeToByteArray(), attributes?.values?.get(1))
    }

    @Test
    fun rejectsNullPlatformAttributeWithoutCallingToArray() {
        val raw = IteratorOnlyMap(
            linkedMapOf<String, ByteArray?>("id" to null),
        )

        assertNull(DiscoveryTxt.copyForJni(raw))
    }
}
