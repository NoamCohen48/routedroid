package dev.routedroid.protocol.message

import dev.routedroid.protocol.Protocol

/** HELLO, Android → host (§4.1). */
data class Hello(
    val protocol: Int,
    val session: String,
    val devicePort: Int,
    /** 32 random bytes, lowercase hex. */
    val clientNonce: String,
    val app: String? = null,
) {
    fun encode(): ByteArray = JsonOut()
        .num("protocol", protocol).str("session", session).num("device_port", devicePort)
        .str("client_nonce", clientNonce)
        .apply { if (app != null) str("app", app) }
        .bytes()

    companion object {
        const val MAX_APP_LEN = 64

        /** `protocol` is not checked here: a mismatch is a session decision (§9). */
        fun decode(body: ByteArray): Hello = Fields.decoding {
            val o = Fields.parse(body)
            val session = Fields.str(o, "session")
            if (!Protocol.validSession(session)) Fields.fail("session", "1-40 characters from [A-Za-z0-9._-]")
            val port = Fields.int(o, "device_port")
            if (port !in 1..65535) Fields.fail("device_port", "must be 1-65535")
            val app = if (o.has("app")) Fields.str(o, "app") else null
            if (app != null && app.length > MAX_APP_LEN) Fields.fail("app", "at most 64 characters")
            Hello(Fields.int(o, "protocol"), session, port, Fields.hex("client_nonce", Fields.str(o, "client_nonce"), 64), app)
        }
    }
}
