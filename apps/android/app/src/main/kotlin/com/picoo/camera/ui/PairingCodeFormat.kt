package com.picoo.camera.ui

/** REQ-PICOO-UI-0001 AC-M-PAIR-01 — `482 917` spacing for mono short code display. */
fun formatPairingCode(code: String): String {
    val digits = code.filter { it.isDigit() }.take(6)
    if (digits.length <= 3) return digits
    return "${digits.take(3)} ${digits.drop(3)}"
}
