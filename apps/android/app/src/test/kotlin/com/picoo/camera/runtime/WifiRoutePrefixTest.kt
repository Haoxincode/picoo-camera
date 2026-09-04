package com.picoo.camera.runtime

import java.net.InetAddress
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class WifiRoutePrefixTest {
    @Test
    fun acceptsOnlyAddressesInsideThePhysicalWifiPrefix() {
        val wifiAddress = InetAddress.getByName("192.168.8.108")

        assertTrue(addressIsInPrefix(InetAddress.getByName("192.168.8.110"), wifiAddress, 24))
        assertFalse(addressIsInPrefix(InetAddress.getByName("192.168.9.110"), wifiAddress, 24))
        assertFalse(addressIsInPrefix(InetAddress.getByName("8.8.8.8"), wifiAddress, 24))
    }

    @Test
    fun rejectsAddressesFromAnotherAddressFamily() {
        assertFalse(
            addressIsInPrefix(
                InetAddress.getByName("fe80::110"),
                InetAddress.getByName("192.168.8.108"),
                24,
            ),
        )
    }
}
