package dev.routedroid.protocol.frame

/** Message type codes and wire names (§3). Values are historical; never reassign. */
enum class MessageType(val code: Int, val wireName: String) {
    HELLO(0x01, "HELLO"),
    HELLO_ACK(0x02, "HELLO_ACK"),
    CONFIGURE_VPN(0x03, "CONFIGURE_VPN"),
    VPN_READY(0x04, "VPN_READY"),
    VPN_ERROR(0x05, "VPN_ERROR"),
    AUTH(0x06, "AUTH"),
    IP_PACKET(0x10, "IP_PACKET"),
    PING(0x20, "PING"),
    PONG(0x21, "PONG"),
    STOP(0x30, "STOP"),
    ERROR(0x7F, "ERROR");

    val emptyBody: Boolean get() = this == PING || this == PONG || this == STOP

    companion object {
        fun fromCode(code: Int): MessageType? = entries.firstOrNull { it.code == code }
        fun fromName(name: String): MessageType? = entries.firstOrNull { it.wireName == name }
    }
}
