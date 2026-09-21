package dev.routedroid.protocol.message

import dev.routedroid.protocol.Protocol

/** Body of ERROR and VPN_ERROR (§4.6). */
data class ErrorBody(val code: String, val message: String, val supported: List<Int>? = null) {
    constructor(code: ErrorCode, message: String) : this(code.wire, message)

    val knownCode: ErrorCode? get() = ErrorCode.parse(code)

    fun encode(): ByteArray = JsonOut()
        .str("code", code).str("message", message)
        .apply { if (supported != null) numList("supported", supported) }
        .bytes()

    companion object {
        const val MAX_MESSAGE_LEN = 512

        fun protocolUnsupported(message: String) =
            ErrorBody(ErrorCode.PROTOCOL_UNSUPPORTED.wire, message, listOf(Protocol.VERSION))

        fun decode(body: ByteArray): ErrorBody = Fields.decoding {
            val o = Fields.parse(body)
            val code = Fields.str(o, "code")
            if (code.isEmpty() || !code.all { it in 'a'..'z' || it == '_' }) Fields.fail("code", "snake_case identifier")
            val msg = Fields.str(o, "message")
            if (msg.length > MAX_MESSAGE_LEN) Fields.fail("message", "at most 512 characters")
            val sup = o.optJSONArray("supported")?.let { a -> (0 until a.length()).map { a.getInt(it) } }
            ErrorBody(code, msg, sup)
        }
    }
}
