package dev.routedroid.protocol.auth

import dev.routedroid.protocol.Protocol

/**
 * The 80-byte record the shell streams to the provider (§7.1), the app's only trusted input
 * before AUTH:
 *
 *     "RDB1"[4] | version u8 = 1 | reserved u8 = 0 | device_port u16be | session[40] | secret[32]
 *
 * Not a data class: the secret must not leak through `copy`, `componentN` or `equals`.
 */
class BootstrapRecord private constructor(val session: String, val devicePort: Int, private val secret: ByteArray) {
    /** Runs [block] with the secret; the array is wiped by [wipe], never copied out. */
    fun <T> withSecret(block: (ByteArray) -> T): T = block(secret)

    fun wipe() = secret.fill(0)

    /** The host's side of the record; the app only decodes. */
    fun encode(): ByteArray {
        val r = ByteArray(LENGTH)
        MAGIC.copyInto(r, 0)
        r[4] = Protocol.VERSION.toByte()
        r[6] = (devicePort ushr 8).toByte()
        r[7] = devicePort.toByte()
        session.toByteArray(Charsets.US_ASCII).copyInto(r, SESSION_AT)
        secret.copyInto(r, SECRET_AT)
        return r
    }

    override fun toString() = "BootstrapRecord(session=$session, devicePort=$devicePort, secret=<redacted>)"

    companion object {
        const val LENGTH = 80
        const val PROVIDER_AUTHORITY = "dev.routedroid.bootstrap"
        const val PROVIDER_URI = "content://$PROVIDER_AUTHORITY/record"
        val MAGIC = "RDB1".toByteArray(Charsets.US_ASCII)
        private const val SESSION_FIELD_LEN = 40
        private const val SESSION_AT = 8
        private const val SECRET_AT = SESSION_AT + SESSION_FIELD_LEN

        /** Takes ownership of [secret]. */
        fun of(session: String, devicePort: Int, secret: ByteArray): BootstrapRecord {
            require(Protocol.validSession(session)) { "invalid session id" }
            require(devicePort in 1..65535) { "invalid device port" }
            require(secret.size == Auth.SECRET_LEN) { "secret is ${Auth.SECRET_LEN} bytes" }
            return BootstrapRecord(session, devicePort, secret)
        }

        /** Null when [bytes] is not a well-formed record; the caller treats that as no record. */
        fun decode(bytes: ByteArray): BootstrapRecord? {
            if (bytes.size != LENGTH) return null
            for (i in MAGIC.indices) if (bytes[i] != MAGIC[i]) return null
            if (bytes[4].toInt() != Protocol.VERSION || bytes[5].toInt() != 0) return null
            val port = ((bytes[6].toInt() and 0xFF) shl 8) or (bytes[7].toInt() and 0xFF)
            if (port == 0) return null
            var end = SESSION_AT
            while (end < SECRET_AT && bytes[end] != 0.toByte()) end++
            for (i in end until SECRET_AT) if (bytes[i] != 0.toByte()) return null
            val session = String(bytes, SESSION_AT, end - SESSION_AT, Charsets.US_ASCII)
            if (!Protocol.validSession(session)) return null
            return BootstrapRecord(session, port, bytes.copyOfRange(SECRET_AT, LENGTH))
        }
    }
}
