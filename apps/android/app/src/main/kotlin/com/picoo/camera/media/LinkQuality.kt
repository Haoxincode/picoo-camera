package com.picoo.camera.media

/**
 * Coarse Wi-Fi / link quality from ReceiverStats (PUC-005 live metrics).
 */
object LinkQuality {
    fun label(packetLoss: Double, rttMs: Double): String =
        when {
            packetLoss >= 0.05 || rttMs >= 120.0 -> "Poor"
            packetLoss >= 0.02 || rttMs >= 60.0 -> "Fair"
            packetLoss > 0.0 || rttMs >= 30.0 -> "Good"
            else -> "Excellent"
        }

    fun formatLine(
        rttMs: Double,
        packetLoss: Double,
        frameAgeMs: Double,
        receiveBitrate: Double,
    ): String {
        val quality = label(packetLoss, rttMs)
        val lossPct = packetLoss * 100.0
        val kbps = (receiveBitrate / 1000.0).toInt()
        return "Link: $quality · RTT ${rttMs.toInt()}ms · loss ${"%.1f".format(lossPct)}% · age ${frameAgeMs.toInt()}ms · rx ${kbps}kbps"
    }
}
