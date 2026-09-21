package dev.routedroid.protocol

/** The four IPv4 header checks applied before injection (§6). */
object Ipv4 {
    /** Null when the packet is acceptable; otherwise a short reason for counters/logs. */
    fun reject(p: ByteArray): String? {
        if (p.size < 20) return "too_short"
        val version = (p[0].toInt() and 0xFF) ushr 4
        if (version != 4) return "version_$version"
        val ihl = p[0].toInt() and 0x0F
        if (ihl < 5) return "ihl_$ihl"
        val total = ((p[2].toInt() and 0xFF) shl 8) or (p[3].toInt() and 0xFF)
        if (total != p.size) return "total_length"
        if (total < ihl * 4) return "header_overrun"
        return null
    }
}
