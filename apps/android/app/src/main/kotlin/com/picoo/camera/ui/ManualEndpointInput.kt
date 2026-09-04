package com.picoo.camera.ui

/** REQ-PICOO-UI-003 / AC-M-MANUAL-01 — editable IPv4 and port field state. */
internal data class ManualEndpointDraft(
    val octets: List<String>,
    val port: String,
) {
    init {
        require(octets.size == IPV4_OCTET_COUNT)
    }

    fun updateOctet(index: Int, value: String): ManualEndpointDraft = copy(
        octets = octets.toMutableList().also { it[index] = value.digits(MAX_OCTET_DIGITS) },
    )

    fun updatePort(value: String): ManualEndpointDraft = copy(port = value.digits(MAX_PORT_DIGITS))

    fun asText(): String = "${octets.joinToString(".")}:$port"

    fun validatedEndpoint(): ManualEndpoint? {
        val values = octets.map { octet ->
            octet.toIntOrNull()?.takeIf { it in 0..255 } ?: return null
        }
        val parsedPort = port.toIntOrNull()?.takeIf { it in 1..65535 } ?: return null
        return ManualEndpoint(values.joinToString("."), parsedPort)
    }

    companion object {
        private const val IPV4_OCTET_COUNT = 4
        private const val MAX_OCTET_DIGITS = 3
        private const val MAX_PORT_DIGITS = 5
        private val DEFAULT_PREFIX = listOf("192", "168")
        const val DEFAULT_PORT = "4433"

        fun from(text: String): ManualEndpointDraft {
            val trimmed = text.trim()
            val separator = trimmed.lastIndexOf(':')
            val hostText = if (separator >= 0) trimmed.substring(0, separator) else trimmed
            val portText = if (separator >= 0) trimmed.substring(separator + 1) else DEFAULT_PORT
            val parsedOctets = hostText.split('.').take(IPV4_OCTET_COUNT)
            val octets = List(IPV4_OCTET_COUNT) { index ->
                parsedOctets.getOrNull(index).orEmpty().digits(MAX_OCTET_DIGITS).ifEmpty {
                    if (hostText.isEmpty()) DEFAULT_PREFIX.getOrNull(index).orEmpty() else ""
                }
            }
            return ManualEndpointDraft(
                octets = octets,
                port = if (separator >= 0) {
                    portText.digits(MAX_PORT_DIGITS)
                } else {
                    DEFAULT_PORT
                },
            )
        }

        fun fromPastedText(text: String): ManualEndpointDraft? {
            val trimmed = text.trim()
            if (trimmed.count { it == ':' } > 1) return null
            val hostText = trimmed.substringBeforeLast(':', trimmed)
            if (hostText.split('.').size != IPV4_OCTET_COUNT) return null
            val draft = from(trimmed)
            return draft.takeIf { it.octets.all(String::isNotEmpty) }
        }

        fun shouldAdvanceOctet(value: String): Boolean {
            val digits = value.digits(MAX_OCTET_DIGITS)
            return digits.length == MAX_OCTET_DIGITS ||
                (digits.length == 2 && (digits.toIntOrNull() ?: 0) > 25)
        }

        private fun String.digits(maxLength: Int): String = filter { it in '0'..'9' }.take(maxLength)
    }
}

internal data class ManualEndpoint(val host: String, val port: Int)
