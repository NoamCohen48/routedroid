package dev.routedroid.protocol.auth

import dev.routedroid.protocol.Protocol

/** The 80-byte record streamed to the provider (§7.1). */
data class BootstrapRecord(val session: String, val secret: ByteArray) {
    fun encode(): ByteArray {
        require(Protocol.validSession(session)) { "invalid session id" }
        require(secret.size == Auth.SECRET_LEN)
        val r = ByteArray(LENGTH)
        MAGIC.copyInto(r, 0)
        r[4] = Protocol.VERSION.toByte()
        session.toByteArray(Charsets.UTF_8).copyInto(r, 8)
        secret.copyInto(r, 48)
        return r
    }

    fun wipe() = secret.fill(0)

    override fun equals(other: Any?) = other is BootstrapRecord && other.session == session && other.secret.contentEquals(secret)
    override fun hashCode() = session.hashCode()
    override fun toString() = "BootstrapRecord(session=$session, secret=<redacted>)"

    companion object {
        const val LENGTH = 80
        const val SESSION_FIELD_LEN = 40
        const val PROVIDER_URI = "content://dev.routedroid.bootstrap/record"
        val MAGIC = "RDB1".toByteArray(Charsets.US_ASCII)

        /** Null when the record is malformed; the caller treats that as "no record". */
        fun decode(bytes: ByteArray): BootstrapRecord? {
            if (bytes.size != LENGTH) return null
            if (!bytes.copyOfRange(0, 4).contentEquals(MAGIC)) return null
            if (bytes[4].toInt() != Protocol.VERSION) return null
            val field = bytes.copyOfRange(8, 8 + SESSION_FIELD_LEN)
            val end = field.indexOf(0).let { if (it < 0) SESSION_FIELD_LEN else it }
            if (field.drop(end).any { it != 0.toByte() }) return null
            val session = String(field, 0, end, Charsets.UTF_8)
            if (!Protocol.validSession(session)) return null
            return BootstrapRecord(session, bytes.copyOfRange(48, LENGTH))
        }
    }
}
