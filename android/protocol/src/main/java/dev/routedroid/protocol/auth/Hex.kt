package dev.routedroid.protocol.auth

object Hex {
    private const val DIGITS = "0123456789abcdef"

    fun encode(b: ByteArray): String {
        val sb = StringBuilder(b.size * 2)
        for (x in b) { val v = x.toInt() and 0xFF; sb.append(DIGITS[v ushr 4]).append(DIGITS[v and 0xF]) }
        return sb.toString()
    }

    /** Strict: lowercase only, even length; null otherwise. */
    fun decode(s: String): ByteArray? {
        if (s.length % 2 != 0) return null
        val out = ByteArray(s.length / 2)
        for (i in out.indices) {
            val hi = DIGITS.indexOf(s[2 * i]); val lo = DIGITS.indexOf(s[2 * i + 1])
            if (hi < 0 || lo < 0) return null
            out[i] = ((hi shl 4) or lo).toByte()
        }
        return out
    }
}
