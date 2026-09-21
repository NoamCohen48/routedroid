package dev.routedroid.protocol

import dev.routedroid.protocol.Fixtures.objects
import dev.routedroid.protocol.message.AuthBody
import dev.routedroid.protocol.message.BodyException
import dev.routedroid.protocol.message.ConfigureVpn
import dev.routedroid.protocol.message.ErrorBody
import dev.routedroid.protocol.message.ErrorCode
import dev.routedroid.protocol.message.Hello
import dev.routedroid.protocol.message.HelloAck
import dev.routedroid.protocol.message.VpnReady
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class MessageFixtureTest {
    private fun body(name: String): ByteArray =
        Fixtures.load("frames.json").getJSONArray("valid").objects().first { it.getString("name") == name }
            .let { Fixtures.hex(it.getString("body_hex")) }

    private fun <T> roundTrip(name: String, decode: (ByteArray) -> T, encode: (T) -> ByteArray) {
        val b = body(name)
        assertArrayEquals(name, b, encode(decode(b)))
    }

    @Test fun fixturesRoundTripByteExact() {
        roundTrip("hello", Hello::decode, Hello::encode)
        roundTrip("hello_minimal", Hello::decode, Hello::encode)
        roundTrip("hello_ack", HelloAck::decode, HelloAck::encode)
        roundTrip("auth", AuthBody::decode, AuthBody::encode)
        roundTrip("configure_vpn", ConfigureVpn::decode, ConfigureVpn::encode)
        roundTrip("vpn_ready", VpnReady::decode, VpnReady::encode)
        roundTrip("vpn_error", ErrorBody::decode, ErrorBody::encode)
        roundTrip("error_auth_failed", ErrorBody::decode, ErrorBody::encode)
        roundTrip("error_protocol_unsupported", ErrorBody::decode, ErrorBody::encode)
    }

    @Test fun errorCodesKnown() {
        val e = ErrorBody.decode(body("error_protocol_unsupported"))
        assertEquals(ErrorCode.PROTOCOL_UNSUPPORTED, e.knownCode)
        assertEquals(listOf(1), e.supported)
        for (c in ErrorCode.entries) assertEquals(c, ErrorCode.parse(c.wire))
    }

    @Test fun unknownFieldsIgnoredMissingRejected() {
        val nonce = "a".repeat(64)
        val ok = Hello.decode("""{"protocol":1,"session":"s","device_port":1,"client_nonce":"$nonce","future":true}""".toByteArray())
        assertEquals("s", ok.session)
        assertThrows(BodyException::class.java) { Hello.decode("""{"protocol":1,"session":"s"}""".toByteArray()) }
        assertThrows(BodyException::class.java) { Hello.decode("not json".toByteArray()) }
    }

    @Test fun fieldRules() {
        val nonce = "a".repeat(64)
        fun hello(session: String, port: Int, n: String) =
            """{"protocol":1,"session":"$session","device_port":$port,"client_nonce":"$n"}""".toByteArray()
        Hello.decode(hello("ok.session_1-", 9000, nonce))
        for (bad in listOf(hello("", 9000, nonce), hello("s".repeat(41), 9000, nonce), hello("bad space", 9000, nonce),
            hello("s", 0, nonce), hello("s", 1, "A".repeat(64)), hello("s", 1, "a".repeat(63))))
            assertThrows(BodyException::class.java) { Hello.decode(bad) }

        val cfg = ConfigureVpn.decode(body("configure_vpn"))
        assertThrows(BodyException::class.java) { ConfigureVpn.decode(cfg.copy(addresses = cfg.addresses + cfg.addresses).encode()) }
        assertThrows(BodyException::class.java) { ConfigureVpn.decode(cfg.copy(routes = emptyList()).encode()) }
        assertThrows(BodyException::class.java) { ConfigureVpn.decode(cfg.copy(mtu = 575).encode()) }
        ConfigureVpn.decode(cfg.copy(mtu = 65535).encode())
        assertThrows(BodyException::class.java) { HelloAck.decode(HelloAck(2, 1400, nonce, nonce).encode()) }
        assertThrows(BodyException::class.java) { VpnReady.decode(VpnReady(listOf("10.0.0.1/33"), 1400).encode()) }
        assertThrows(BodyException::class.java) { VpnReady.decode(VpnReady(listOf("10.0.0.1"), 1400).encode()) }
    }
}
