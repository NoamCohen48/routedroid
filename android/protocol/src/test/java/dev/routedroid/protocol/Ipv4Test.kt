package dev.routedroid.protocol

import dev.routedroid.protocol.Fixtures.objects
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class Ipv4Test {
    private val packets = Fixtures.load("frames.json").getJSONArray("valid").objects()
        .filter { it.getInt("type") == 0x10 }.map { it.getString("name") to Fixtures.hex(it.getString("body_hex")) }

    @Test fun fixturePacketsPass() {
        assertTrue(packets.size >= 3)
        for ((name, p) in packets) assertNull(name, Ipv4.reject(p))
    }

    @Test fun eachRuleEnforced() {
        val good = packets.first().second
        assertEquals("too_short", Ipv4.reject(good.copyOf(19)))
        assertEquals("version_6", Ipv4.reject(good.copyOf().also { it[0] = 0x65 }))
        assertEquals("ihl_4", Ipv4.reject(good.copyOf().also { it[0] = 0x44 }))
        assertEquals("total_length", Ipv4.reject(good + byteArrayOf(0)))
        assertEquals("header_overrun", Ipv4.reject(good.copyOf().also { it[0] = 0x4f }))
    }
}
