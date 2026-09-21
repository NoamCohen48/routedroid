package dev.routedroid.protocol

import dev.routedroid.protocol.Fixtures.objects
import dev.routedroid.protocol.frame.Frame
import dev.routedroid.protocol.frame.FrameException
import dev.routedroid.protocol.frame.FrameReader
import dev.routedroid.protocol.frame.MessageType
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream

class FrameFixtureTest {
    private val f = Fixtures.load("frames.json")
    private val mtu = f.getInt("mtu")

    @Test fun constantsMatch() {
        assertEquals(Protocol.VERSION, f.getInt("version"))
        assertEquals(Protocol.DEFAULT_MTU, mtu)
        assertEquals(Protocol.MAX_CONTROL_BODY, f.getInt("control_limit"))
    }

    @Test fun validFramesEncodeAndDecodeExactly() {
        val valid = f.getJSONArray("valid").objects()
        assertTrue(valid.size >= 16)
        for (v in valid) {
            val name = v.getString("name")
            val type = MessageType.fromCode(v.getInt("type")) ?: error("$name: type")
            val body = Fixtures.hex(v.getString("body_hex"))
            val wire = Fixtures.hex(v.getString("wire_hex"))
            assertArrayEquals(name, wire, Frame(type, body).encode())
            val (d, used) = Frame.decode(wire, mtu)
            assertEquals(name, wire.size, used)
            assertEquals(name, type, d.type)
            assertArrayEquals(name, body, d.body)
        }
    }

    @Test fun invalidFramesRejectedWithNamedCode() {
        val invalid = f.getJSONArray("invalid").objects()
        assertTrue(invalid.size >= 20)
        for (i in invalid) {
            val name = i.getString("name")
            try {
                Frame.decode(Fixtures.hex(i.getString("wire_hex")), mtu)
                fail("$name: accepted")
            } catch (e: FrameException) {
                assertEquals(name, i.getString("error"), e.code)
            }
        }
    }

    @Test fun readerStreamsAllValidThenCleanEof() {
        val valid = f.getJSONArray("valid").objects()
        val stream = ByteArrayOutputStream()
        valid.forEach { stream.write(Fixtures.hex(it.getString("wire_hex"))) }
        val r = FrameReader(ByteArrayInputStream(stream.toByteArray()), mtu)
        for (v in valid) assertArrayEquals(v.getString("name"), Fixtures.hex(v.getString("body_hex")), r.read()!!.body)
        assertNull(r.read())
    }

    @Test fun readerRejectsHostileLengthBeforeAllocating() {
        val hostile = f.getJSONArray("invalid").objects().first { it.getString("name") == "control_hostile_length" }
        val input = ByteArrayInputStream(Fixtures.hex(hostile.getString("wire_hex")))
        try { FrameReader(input, mtu).read(); fail("accepted") } catch (e: FrameException) {
            assertEquals(FrameException.CONTROL_BODY_TOO_LARGE, e.code)
        }
        assertEquals("nothing read past the header", 0, input.available())
    }
}
