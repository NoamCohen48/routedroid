package dev.routedroid.protocol.net

/** The four IPv4 header checks applied before a packet is injected (§6). */
object Ipv4Packet {
    /** Null when the packet at `p[off, off+len)` may be injected; otherwise a short reason. */
    fun reject(p: ByteArray, off: Int = 0, len: Int = p.size - off): String? {
        if (len < 20) return "too_short"
        val version = (p[off].toInt() and 0xFF) ushr 4
        if (version != 4) return "version_$version"
        val ihl = p[off].toInt() and 0x0F
        if (ihl < 5) return "ihl_$ihl"
        val total = ((p[off + 2].toInt() and 0xFF) shl 8) or (p[off + 3].toInt() and 0xFF)
        if (total != len) return "total_length"
        if (total < ihl * 4) return "header_overrun"
        return null
    }
}
