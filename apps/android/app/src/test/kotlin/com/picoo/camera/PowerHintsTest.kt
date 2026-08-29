package com.picoo.camera

import android.os.PowerManager
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** REQ-PICOO-UI-005 */
class PowerHintsTest {
    @Test
    fun batteryHint_below_20_percent() {
        assertEquals(
            "Low battery (15%) — streaming may stop",
            PowerHints.batteryHint(15),
        )
        assertNull(PowerHints.batteryHint(50))
        assertNull(PowerHints.batteryHint(null))
    }

    @Test
    fun thermalHint_severe_and_critical() {
        assertEquals(
            "Device overheating — reduce resolution or stop streaming",
            PowerHints.thermalHint(PowerManager.THERMAL_STATUS_SEVERE),
        )
        assertEquals(
            "Critical thermal state — stop streaming",
            PowerHints.thermalHint(PowerManager.THERMAL_STATUS_CRITICAL),
        )
        assertNull(PowerHints.thermalHint(PowerManager.THERMAL_STATUS_NONE))
    }

    @Test
    fun combine_joins_non_null() {
        assertEquals(
            "Low battery (10%) — streaming may stop · Device overheating — reduce resolution or stop streaming",
            PowerHints.combine(
                PowerHints.batteryHint(10),
                PowerHints.thermalHint(PowerManager.THERMAL_STATUS_SEVERE),
            ),
        )
    }

    @Test
    fun thermalSevereForces720p() {
        assertEquals(true, PowerHints.shouldForce720p(PowerManager.THERMAL_STATUS_SEVERE))
        assertEquals(true, PowerHints.shouldForce720p(PowerManager.THERMAL_STATUS_CRITICAL))
        assertEquals(false, PowerHints.shouldForce720p(PowerManager.THERMAL_STATUS_NONE))
        assertEquals(false, PowerHints.shouldForce720p(PowerManager.THERMAL_STATUS_MODERATE))
    }
}
