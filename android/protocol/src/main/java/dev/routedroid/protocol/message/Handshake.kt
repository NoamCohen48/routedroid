package dev.routedroid.protocol.message

import dev.routedroid.protocol.Protocol
import dev.routedroid.protocol.json.JsonWriter

// HELLO, HELLO_ACK and AUTH (§4.1–4.3). Each codec is symmetric: the app encodes what it
// sends, and the decoders of what only the host receives are kept so the fixtures can be
// round-tripped byte for byte.

private const val HEX_LEN = 64

/** HELLO, Android → host. */
data class Hello(val protocol: Int, val session: String, val devicePort: Int, val clientNonce: String, val app: String? = null) {
    fun encode(): ByteArray = JsonWriter()
        .int("protocol", protocol).string("session", session).int("device_port", devicePort)
        .string("client_nonce", clientNonce)
        .apply { if (app != null) string("app", app) }
        .bytes()

    companion object {
        const val MAX_APP_LEN = 64

        fun decode(body: ByteArray): Hello {
            val f = Fields.parse(body)
            val session = f.string("session")
            if (!Protocol.validSession(session)) Fields.fail("session", "1-40 characters from [A-Za-z0-9._-]")
            val app = f.optionalString("app")
            if (app != null && Fields.codePoints(app) > MAX_APP_LEN) Fields.fail("app", "at most $MAX_APP_LEN characters")
            return Hello(f.int("protocol", 0..Int.MAX_VALUE), session, f.int("device_port", 1..65535), f.hex("client_nonce", HEX_LEN), app)
        }
    }
}

/** HELLO_ACK, host → Android. */
data class HelloAck(val protocol: Int, val mtu: Int, val hostNonce: String, val hostProof: String) {
    fun encode(): ByteArray = JsonWriter()
        .int("protocol", protocol).int("mtu", mtu).string("host_nonce", hostNonce).string("host_proof", hostProof)
        .bytes()

    companion object {
        fun decode(body: ByteArray): HelloAck {
            val f = Fields.parse(body)
            val protocol = f.int("protocol", 0..255)
            if (protocol != Protocol.VERSION) Fields.fail("protocol", "must be ${Protocol.VERSION}")
            return HelloAck(protocol, f.mtu("mtu"), f.hex("host_nonce", HEX_LEN), f.hex("host_proof", HEX_LEN))
        }
    }
}

/** AUTH, Android → host. */
data class AuthBody(val androidProof: String) {
    fun encode(): ByteArray = JsonWriter().string("android_proof", androidProof).bytes()

    companion object {
        fun decode(body: ByteArray): AuthBody = AuthBody(Fields.parse(body).hex("android_proof", HEX_LEN))
    }
}
