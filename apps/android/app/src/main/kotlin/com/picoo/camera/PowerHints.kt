package com.picoo.camera

import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.os.Build
import android.os.PowerManager

/**
 * Battery / thermal hints while streaming — REQ-PICOO-UI-005 / PUC-005.
 */
object PowerHints {
    fun batteryPercent(level: Int, scale: Int): Int? {
        if (level < 0 || scale <= 0) return null
        return (level * 100) / scale
    }

    fun batteryHint(percent: Int?): String? {
        val pct = percent ?: return null
        return if (pct < 20) "Low battery ($pct%) — streaming may stop" else null
    }

    /** Map [PowerManager] thermal status codes to a user-visible string. */
    fun thermalHint(status: Int): String? =
        when (status) {
            PowerManager.THERMAL_STATUS_SEVERE ->
                "Device overheating — reduce resolution or stop streaming"
            PowerManager.THERMAL_STATUS_CRITICAL,
            PowerManager.THERMAL_STATUS_EMERGENCY,
            PowerManager.THERMAL_STATUS_SHUTDOWN,
            -> "Critical thermal state — stop streaming"
            else -> null
        }

    fun combine(battery: String?, thermal: String?): String =
        listOfNotNull(battery, thermal).joinToString(" · ")

    fun readHint(context: Context): String {
        val filter = IntentFilter(Intent.ACTION_BATTERY_CHANGED)
        val battery = context.registerReceiver(null, filter)
        val level = battery?.getIntExtra(BatteryManager.EXTRA_LEVEL, -1) ?: -1
        val scale = battery?.getIntExtra(BatteryManager.EXTRA_SCALE, -1) ?: -1
        val batteryMsg = batteryHint(batteryPercent(level, scale))

        val thermalMsg =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                val pm = context.getSystemService(Context.POWER_SERVICE) as PowerManager
                thermalHint(pm.currentThermalStatus)
            } else {
                null
            }

        return combine(batteryMsg, thermalMsg)
    }
}
