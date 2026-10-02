package dev.routedroid.protocol

import dev.routedroid.protocol.Fixtures.hex
import dev.routedroid.protocol.Fixtures.int
import dev.routedroid.protocol.Fixtures.objects
import dev.routedroid.protocol.Fixtures.str
import dev.routedroid.protocol.frame.ByteSource
import dev.routedroid.protocol.frame.Frame
import dev.routedroid.protocol.frame.FrameException
import dev.routedroid.protocol.frame.FrameHeader
import dev.routedroid.protocol.frame.FrameReader
import dev.routedroid.protocol.frame.MessageType
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.ByteArrayOutputStream

class FrameFixtureTest {
    private val f = Fixtures.load("frames.json")
    private val mtu = f.int("mtu")
    private val valid = f.objects("valid")

    @Test fun constantsMatch() {
        assertEquals(Protocol.VERSION, f.int("version"))
        assertEquals(Protocol.MAX_CONTROL_BODY, f.int("control_limit"))
    }

    @Test fun validFramesEncodeAndReadExactly() {
        assertTrue(valid.size >= 16)
        for (v in valid) {
            val name = v.str("name")
            val type = MessageType.fromCode(v.int("type")) ?: error("$name: type")
            val wire = v.hex("wire_hex")
            assertArrayEquals(name, wire, Frame(type, v.hex("body_hex")).encode())
            val frame = FrameReader(Fixtures.source(wire)).frame(mtu)!!
            assertEquals(name, type, frame.type)
            assertArrayEquals(name, v.hex("body_hex"), frame.body)
        }
    }

    @Test fun invalidFramesRejectedWithNamedCode() {
        val invalid = f.objects("invalid")
        assertTrue(invalid.size >= 20)
        for (i in invalid) {
            val name = i.str("name")
            try {
                val frame = FrameReader(Fixtures.source(i.hex("wire_hex"))).frame(mtu)
                // An empty stream is a clean end to a reader; the fixture calls it truncated.
                if (frame == null && i.hex("wire_hex").isEmpty()) continue
                fail("$name: accepted")
            } catch (e: FrameException) {
                assertEquals(name, i.str("error"), e.code)
            }
        }
    }

    @Test fun streamOfAllValidFramesInSmallReadsThenCleanEnd() {
        val stream = ByteArrayOutputStream()
        valid.forEach { stream.write(it.hex("wire_hex")) }
        val r = FrameReader(Fixtures.source(stream.toByteArray(), chunk = 3))
        for (v in valid) assertArrayEquals(v.str("name"), v.hex("body_hex"), r.frame(mtu)!!.body)
        assertNull(r.frame(mtu))
    }

    @Test fun hostileLengthRejectedBeforeAnyBodyIsRead() {
        val hostile = f.objects("invalid").first { it.str("name") == "control_hostile_length" }
        val wire = hostile.hex("wire_hex") + ByteArray(16)
        var read = 0
        val source = Fixtures.source(wire).let { s -> ByteSource { d -> s.read(d).also { if (it > 0) read += it } } }
        try { FrameReader(source).next(mtu); fail("accepted") } catch (e: FrameException) {
            assertEquals(FrameException.CONTROL_BODY_TOO_LARGE, e.code)
        }
        assertEquals("nothing read past the header", Protocol.HEADER_LEN, read)
    }

    @Test fun beforeNegotiationOnlyTheAbsolutePacketLimitApplies() {
        val big = ByteArray(Protocol.HEADER_LEN).also { FrameHeader.write(it, 0, MessageType.IP_PACKET, 9000) }
        assertTrue(FrameReader(Fixtures.source(big)).next(null))
        assertFalse(FrameReader(Fixtures.source(ByteArray(0))).next(null))
    }

    @Test fun everyCodeLooksUpItsType() {
        for (t in MessageType.entries) assertEquals(t, MessageType.fromCode(t.code))
        assertNull(MessageType.fromCode(0x11))
        assertNull(MessageType.fromCode(-1))
        assertNull(MessageType.fromCode(256))
    }
}
