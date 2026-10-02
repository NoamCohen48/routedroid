package dev.routedroid.protocol

import dev.routedroid.protocol.Fixtures.hex
import dev.routedroid.protocol.Fixtures.int
import dev.routedroid.protocol.Fixtures.objects
import dev.routedroid.protocol.Fixtures.str
import dev.routedroid.protocol.json.JsonObject
import dev.routedroid.protocol.message.BodyException
import dev.routedroid.protocol.message.ConfigureVpn
import dev.routedroid.protocol.message.ErrorBody
import dev.routedroid.protocol.message.HelloAck
import dev.routedroid.protocol.message.VpnReady
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.fail
import org.junit.Test

/** fixtures/bodies.json: the strict JSON and field rules, judged identically by the host. */
class BodyFixtureTest {
    private val f = Fixtures.load("bodies.json")
    private val mtu = f.int("mtu")

    private fun decode(v: JsonObject): Any = when (val kind = v.str("kind")) {
        "configure_vpn" -> ConfigureVpn.decode(v.hex("body_hex"), mtu)
        "error" -> ErrorBody.decode(v.hex("body_hex"))
        "hello_ack" -> HelloAck.decode(v.hex("body_hex"))
        "vpn_ready" -> VpnReady.decode(v.hex("body_hex")).also {
            if (it.mtu != mtu) throw BodyException("mtu ${it.mtu} is not the negotiated $mtu")
        }
        else -> error("unknown kind $kind")
    }

    @Test fun validBodiesDecode() {
        for (v in f.objects("valid")) decode(v)
    }

    @Test fun invalidBodiesRefused() {
        val cases = f.objects("invalid")
        assertEquals(47, cases.size)
        for (v in cases) {
            try {
                decode(v)
                fail("${v.str("name")} decoded")
            } catch (_: BodyException) {
            }
        }
    }

    @Test fun decodedValuesAreExact() {
        val byName = f.objects("valid").associateBy { it.str("name") }
        val escaped = ConfigureVpn.decode(byName.getValue("escaped_slash_and_unicode").hex("body_hex"), mtu)
        assertEquals("10.100.102.222/32", escaped.address.toString())
        val emoji = ConfigureVpn.decode(byName.getValue("session_name_64_code_points").hex("body_hex"), mtu)
        assertEquals(64, emoji.sessionName.codePointCount(0, emoji.sessionName.length))
        assertEquals(emptyList<Any>(), ConfigureVpn.decode(byName.getValue("no_dns").hex("body_hex"), mtu).dns)
        val unknown = ErrorBody.decode(byName.getValue("unknown_error_code").hex("body_hex"))
        assertEquals("from_the_future", unknown.code)
        assertNull(unknown.knownCode)
    }
}
