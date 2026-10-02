package dev.routedroid.protocol

import dev.routedroid.protocol.Fixtures.hex
import dev.routedroid.protocol.Fixtures.int
import dev.routedroid.protocol.Fixtures.objects
import dev.routedroid.protocol.Fixtures.str
import dev.routedroid.protocol.net.Ipv4Address
import dev.routedroid.protocol.net.Ipv4Packet
import dev.routedroid.protocol.net.Ipv4Prefix
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class Ipv4Test {
    private val packets: List<Pair<String, ByteArray>> = Fixtures.load("frames.json").objects("valid").filter { it.int("type") == 0x10 }
        .map { it.str("name") to it.hex("body_hex") }

    @Test fun fixturePacketsPass() {
        assertTrue(packets.size >= 3)
        for ((name, p) in packets) assertNull(name, Ipv4Packet.reject(p))
    }

    @Test fun eachPacketRuleEnforced() {
        val good = packets.first().second
        assertEquals("too_short", Ipv4Packet.reject(good.copyOf(19)))
        assertEquals("version_6", Ipv4Packet.reject(good.copyOf().also { it[0] = 0x65 }))
        assertEquals("ihl_4", Ipv4Packet.reject(good.copyOf().also { it[0] = 0x44 }))
        assertEquals("total_length", Ipv4Packet.reject(good + byteArrayOf(0)))
        assertEquals("header_overrun", Ipv4Packet.reject(good.copyOf().also { it[0] = 0x4f }))
        assertNull("at an offset", Ipv4Packet.reject(byteArrayOf(9, 9) + good, 2, good.size))
    }

    @Test fun dottedQuadIsStrict() {
        assertEquals("10.0.0.255", Ipv4Address.parse("10.0.0.255").toString())
        assertEquals("0.0.0.0", Ipv4Address.parse("0.0.0.0").toString())
        for (bad in listOf("", "10.0.0", "10.0.0.0.1", "10.0.0.256", "010.0.0.1", "10.0.0.-1", "10.0.0.1 ", "١٠.0.0.1", "1e1.0.0.1"))
            assertNull(bad, Ipv4Address.parse(bad))
    }

    @Test fun unicastAndCanonical() {
        for (a in listOf("0.1.2.3", "127.0.0.1", "224.0.0.1", "239.255.255.255", "240.0.0.1", "255.255.255.255"))
            assertFalse(a, Ipv4Address.parse(a)!!.isUnicastHost)
        for (a in listOf("1.0.0.0", "10.0.0.1", "126.255.255.255", "128.0.0.1", "223.255.255.255"))
            assertTrue(a, Ipv4Address.parse(a)!!.isUnicastHost)
        fun p(a: String, n: Int) = Ipv4Prefix(Ipv4Address.parse(a)!!, n)
        assertTrue(p("0.0.0.0", 0).isCanonical)
        assertTrue(p("10.0.0.0", 8).isCanonical)
        assertFalse(p("10.0.0.1", 8).isCanonical)
        assertFalse(p("0.0.0.1", 0).isCanonical)
        assertTrue(p("10.0.0.1", 32).isCanonical)
        assertFalse(p("10.0.0.1", 31).isCanonical)
    }
}
