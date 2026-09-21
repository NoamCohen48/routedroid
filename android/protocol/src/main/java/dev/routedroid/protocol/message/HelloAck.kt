package dev.routedroid.protocol.message

import dev.routedroid.protocol.Protocol

/** HELLO_ACK, host → Android (§4.2). */
data class HelloAck(val protocol: Int, val mtu: Int, val hostNonce: String, val hostProof: String) {
    fun encode(): ByteArray = JsonOut()
        .num("protocol", protocol).num("mtu", mtu).str("host_nonce", hostNonce).str("host_proof", hostProof)
        .bytes()

    companion object {
        fun decode(body: ByteArray): HelloAck {
            val o = Fields.parse(body)
            val protocol = Fields.int(o, "protocol")
            if (protocol != Protocol.VERSION) Fields.fail("protocol", "must be ${Protocol.VERSION}")
            return HelloAck(
                protocol,
                Fields.mtu("mtu", Fields.int(o, "mtu")),
                Fields.hex("host_nonce", Fields.str(o, "host_nonce"), 64),
                Fields.hex("host_proof", Fields.str(o, "host_proof"), 64),
            )
        }
    }
}
