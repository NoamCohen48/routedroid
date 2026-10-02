package dev.routedroid.protocol.frame

/** Message type codes and wire names (§3). Values are historical; never reassign. */
enum class MessageType(val code: Int) {
    HELLO(0x01),
    HELLO_ACK(0x02),
    CONFIGURE_VPN(0x03),
    VPN_READY(0x04),
    VPN_ERROR(0x05),
    AUTH(0x06),
    IP_PACKET(0x10),
    PING(0x20),
    PONG(0x21),
    STOP(0x30),
    ERROR(0x7F);

    /** PING, PONG and STOP carry no body. */
    val emptyBody: Boolean get() = this == PING || this == PONG || this == STOP

    companion object {
        private val byCode = arrayOfNulls<MessageType>(256).also { t -> entries.forEach { t[it.code] = it } }

        /** Constant time and allocation free: it runs once per received frame. */
        fun fromCode(code: Int): MessageType? = if (code in 0..255) byCode[code] else null
    }
}
