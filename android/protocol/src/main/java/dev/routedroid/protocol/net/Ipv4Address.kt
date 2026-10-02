package dev.routedroid.protocol.net

/** An IPv4 address, parsed once at the edge from the strict dotted-quad text of §4.4. */
@JvmInline
value class Ipv4Address(val bits: Int) {
    fun octet(i: Int): Int = (bits ushr (24 - 8 * i)) and 0xFF

    fun toBytes(): ByteArray = ByteArray(4) { octet(it).toByte() }

    /** §4.4: outside 0/8, 127/8, 224/4 (multicast) and 240/4 (reserved, broadcast). */
    val isUnicastHost: Boolean
        get() = octet(0) != 0 && octet(0) != 127 && octet(0) < 224

    override fun toString() = "${octet(0)}.${octet(1)}.${octet(2)}.${octet(3)}"

    companion object {
        /** Four parts of 1–3 ASCII digits, each 0–255, no leading zeros; null otherwise. */
        fun parse(text: String): Ipv4Address? {
            val parts = text.split('.')
            if (parts.size != 4) return null
            var bits = 0
            for (p in parts) {
                if (p.isEmpty() || p.length > 3 || !p.all { it in '0'..'9' }) return null
                if (p.length > 1 && p[0] == '0') return null
                val v = p.toInt()
                if (v > 255) return null
                bits = (bits shl 8) or v
            }
            return Ipv4Address(bits)
        }
    }
}

/** `address/length`, as CONFIGURE_VPN carries addresses and routes. */
data class Ipv4Prefix(val address: Ipv4Address, val length: Int) {
    init { require(length in 0..32) { "prefix length $length" } }

    /** No address bits set past the prefix: `10.0.0.0/8`, not `10.0.0.1/8`. */
    val isCanonical: Boolean
        get() = length == 32 || (address.bits and (-1 ushr length)) == 0

    override fun toString() = "$address/$length"
}
