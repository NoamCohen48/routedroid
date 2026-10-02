package dev.routedroid.protocol

import dev.routedroid.protocol.Fixtures.hex
import dev.routedroid.protocol.Fixtures.int
import dev.routedroid.protocol.Fixtures.objects
import dev.routedroid.protocol.Fixtures.str
import dev.routedroid.protocol.message.AuthBody
import dev.routedroid.protocol.message.ConfigureVpn
import dev.routedroid.protocol.message.ErrorBody
import dev.routedroid.protocol.message.ErrorCode
import dev.routedroid.protocol.message.Hello
import dev.routedroid.protocol.message.HelloAck
import dev.routedroid.protocol.message.VpnReady
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Test

/** Every control body in frames.json decodes and re-encodes byte for byte, as the host writes it. */
class MessageFixtureTest {
    private val f = Fixtures.load("frames.json")
    private val mtu = f.int("mtu")
    private val bodies: Map<String, ByteArray> = f.objects("valid").associate { it.str("name") to it.hex("body_hex") }

    private fun roundTrip(name: String, codec: (ByteArray) -> ByteArray) {
        val body = bodies.getValue(name)
        assertArrayEquals(name, body, codec(body))
    }

    @Test fun controlBodiesRoundTrip() {
        roundTrip("hello") { Hello.decode(it).encode() }
        roundTrip("hello_minimal") { Hello.decode(it).encode() }
        roundTrip("hello_ack") { HelloAck.decode(it).encode() }
        roundTrip("auth") { AuthBody.decode(it).encode() }
        roundTrip("configure_vpn") { ConfigureVpn.decode(it, mtu).encode() }
        roundTrip("vpn_ready") { VpnReady.decode(it).encode() }
        roundTrip("vpn_error") { ErrorBody.decode(it).encode() }
        roundTrip("error_auth_failed") { ErrorBody.decode(it).encode() }
        roundTrip("error_protocol_unsupported") { ErrorBody.decode(it).encode() }
    }

    @Test fun knownCodesParse() {
        assertEquals(ErrorCode.AUTH_FAILED, ErrorBody.decode(bodies.getValue("error_auth_failed")).knownCode)
        assertEquals(listOf(1), ErrorBody.decode(bodies.getValue("error_protocol_unsupported")).supported)
    }

    @Test fun errorMessagesAreCutOnCodePoints() {
        val long = "\uD83D\uDE00".repeat(600)
        val cut = ErrorBody.of(ErrorCode.INTERNAL, long).message
        assertEquals(512, cut.codePointCount(0, cut.length))
        assertEquals(1024, cut.length)
        assertEquals("short", ErrorBody.of(ErrorCode.INTERNAL, "short").message)
    }

    @Test fun writerEscapesLikeTheHost() {
        val text = "q\" b\\ n\n t\t c\u0001 /é"
        val body = ErrorBody(ErrorCode.INTERNAL.wire, text).encode()
        assertEquals("{\"code\":\"internal\",\"message\":\"q\\\" b\\\\ n\\n t\\t c\\u0001 /é\"}", String(body, Charsets.UTF_8))
        assertEquals(text, ErrorBody.decode(body).message)
    }
}
