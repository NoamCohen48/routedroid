package dev.routedroid.protocol.message

import dev.routedroid.protocol.json.JsonWriter

/** Body of ERROR and VPN_ERROR (§4.6). */
class ErrorBody(val code: String, val message: String, val supported: List<Int>? = null) {
    val knownCode: ErrorCode? get() = ErrorCode.parse(code)

    fun encode(): ByteArray = JsonWriter()
        .string("code", code).string("message", message)
        .apply { if (supported != null) ints("supported", supported) }
        .bytes()

    override fun toString() = "$code: $message"

    companion object {
        const val MAX_MESSAGE_LEN = 512

        /** A body to send: [message] is cut to [MAX_MESSAGE_LEN] code points, never mid-pair. */
        fun of(code: ErrorCode, message: String): ErrorBody {
            val cut = if (Fields.codePoints(message) <= MAX_MESSAGE_LEN) message
            else message.substring(0, message.offsetByCodePoints(0, MAX_MESSAGE_LEN))
            return ErrorBody(code.wire, cut)
        }

        fun decode(body: ByteArray): ErrorBody {
            val f = Fields.parse(body)
            val code = f.string("code")
            if (code.isEmpty() || !code.all { it in 'a'..'z' || it == '_' }) Fields.fail("code", "snake_case identifier")
            val message = f.string("message")
            if (Fields.codePoints(message) > MAX_MESSAGE_LEN) Fields.fail("message", "at most $MAX_MESSAGE_LEN characters")
            val supported = f.optionalList("supported")?.map { Fields.int("supported", it, 0..255) }
            return ErrorBody(code, message, supported)
        }
    }
}
