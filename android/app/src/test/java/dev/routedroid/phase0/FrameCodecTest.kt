package dev.routedroid.phase0

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/**
 * Golden vectors for the Phase 0 frame header (protocol/phase0-draft.md). Expected bytes are
 * written out by hand here and never derived from FrameCodec.
 */
class FrameCodecTest {

    private fun hex(s: String): ByteArray =
        s.split(" ").filter { it.isNotEmpty() }.map { it.toInt(16).toByte() }.toByteArray()

    private inline fun assertRejected(what: String, block: () -> Unit) {
        try {
            block()
            fail("$what should have been rejected")
        } catch (_: FrameCodec.FrameException) {
            // expected
        }
    }

    /** A syntactically valid minimal IPv4 header (IHL=5) with total_length = [totalLength]. */
    private fun ipv4(totalLength: Int, actualLength: Int = totalLength): ByteArray {
        val p = ByteArray(actualLength)
        p[0] = 0x45
        p[2] = (totalLength ushr 8).toByte()
        p[3] = totalLength.toByte()
        p[8] = 64 // ttl
        p[9] = 1 // icmp
        return p
    }

    @Test
    fun encodeHello_goldenHeaderAndBody() {
        // {"protocol":0,"session":"s","device_port":9000} is 47 bytes = 0x2F.
        val body = "{\"protocol\":0,\"session\":\"s\",\"device_port\":9000}".toByteArray(Charsets.US_ASCII)
        assertEquals(47, body.size)

        val frame = FrameCodec.encodeControl(FrameCodec.Type.HELLO, body)

        val expectedHeader = hex("00 00 00 2F 00 01 00 00")
        assertArrayEquals(expectedHeader, frame.copyOfRange(0, 8))
        assertArrayEquals(body, frame.copyOfRange(8, frame.size))
        assertEquals(8 + 47, frame.size)
    }

    @Test
    fun encodeEmptyControls_golden() {
        assertArrayEquals(hex("00 00 00 00 00 20 00 00"), FrameCodec.encodeEmpty(FrameCodec.Type.PING))
        assertArrayEquals(hex("00 00 00 00 00 21 00 00"), FrameCodec.encodeEmpty(FrameCodec.Type.PONG))
        assertArrayEquals(hex("00 00 00 00 00 30 00 00"), FrameCodec.encodeEmpty(FrameCodec.Type.STOP))
    }

    @Test
    fun encodeHeader_ipPacketIntoOffsetBuffer() {
        val buf = ByteArray(16)
        FrameCodec.encodeHeader(buf, 4, 1400, FrameCodec.Type.IP_PACKET)
        assertArrayEquals(hex("00 00 05 78 00 10 00 00"), buf.copyOfRange(4, 12))
    }

    @Test
    fun decode_validHeaders() {
        val h1 = FrameCodec.decodeHeader(hex("00 00 00 2F 00 02 00 00"), 0, 1400)
        assertEquals(FrameCodec.Type.HELLO_ACK, h1.type)
        assertEquals(47, h1.bodyLength)

        val h2 = FrameCodec.decodeHeader(hex("00 00 05 78 00 10 00 00"), 0, 1400)
        assertEquals(FrameCodec.Type.IP_PACKET, h2.type)
        assertEquals(1400, h2.bodyLength)

        val h3 = FrameCodec.decodeHeader(hex("00 00 00 00 00 30 00 00"), 0, 1400)
        assertEquals(FrameCodec.Type.STOP, h3.type)
        assertEquals(0, h3.bodyLength)

        // Header at a nonzero offset.
        val h4 = FrameCodec.decodeHeader(hex("FF FF 00 00 00 00 00 20 00 00"), 2, 1400)
        assertEquals(FrameCodec.Type.PING, h4.type)
    }

    @Test
    fun decode_rejectsNonzeroFlags() {
        assertRejected("flags 0x0001") { FrameCodec.decodeHeader(hex("00 00 00 2F 00 02 00 01"), 0, 1400) }
        assertRejected("flags 0x8000") { FrameCodec.decodeHeader(hex("00 00 00 2F 00 02 80 00"), 0, 1400) }
        assertRejected("flags on PING") { FrameCodec.decodeHeader(hex("00 00 00 00 00 20 00 01"), 0, 1400) }
    }

    @Test
    fun decode_rejectsWrongVersion() {
        assertRejected("version 1") { FrameCodec.decodeHeader(hex("00 00 00 2F 01 02 00 00"), 0, 1400) }
        assertRejected("version 0xFF") { FrameCodec.decodeHeader(hex("00 00 00 2F FF 02 00 00"), 0, 1400) }
    }

    @Test
    fun decode_rejectsUnknownType() {
        assertRejected("type 0x00") { FrameCodec.decodeHeader(hex("00 00 00 10 00 00 00 00"), 0, 1400) }
        assertRejected("type 0x11") { FrameCodec.decodeHeader(hex("00 00 00 40 00 11 00 00"), 0, 1400) }
    }

    @Test
    fun decode_rejectsOversizeBodies() {
        // Control: 65536 is the max; 65537 = 0x00010001 must fail.
        FrameCodec.decodeHeader(hex("00 01 00 00 00 02 00 00"), 0, 1400)
        assertRejected("control 65537") { FrameCodec.decodeHeader(hex("00 01 00 01 00 02 00 00"), 0, 1400) }

        // IP_PACKET: <= mtu and > 20.
        FrameCodec.decodeHeader(hex("00 00 00 15 00 10 00 00"), 0, 1400) // 21 ok
        assertRejected("packet 1401 > mtu 1400") { FrameCodec.decodeHeader(hex("00 00 05 79 00 10 00 00"), 0, 1400) }
        assertRejected("packet 20 (not > 20)") { FrameCodec.decodeHeader(hex("00 00 00 14 00 10 00 00"), 0, 1400) }
        assertRejected("packet 0") { FrameCodec.decodeHeader(hex("00 00 00 00 00 10 00 00"), 0, 1400) }

        // Empty controls must be empty; JSON controls must be nonempty.
        assertRejected("PING with body") { FrameCodec.decodeHeader(hex("00 00 00 01 00 20 00 00"), 0, 1400) }
        assertRejected("HELLO_ACK with empty body") { FrameCodec.decodeHeader(hex("00 00 00 00 00 02 00 00"), 0, 1400) }
    }

    @Test
    fun decode_maxLengthDoesNotAllocate() {
        // 0xFFFFFFFF must be rejected purely from the header, with no allocation. If the codec
        // tried to allocate, this would fail with OutOfMemoryError / NegativeArraySize rather
        // than FrameException.
        for (type in listOf(0x01, 0x02, 0x03, 0x04, 0x05, 0x10, 0x20, 0x21, 0x30, 0x7F)) {
            val hdr = hex("FF FF FF FF 00 %02X 00 00".format(type))
            val before = Runtime.getRuntime().totalMemory() - Runtime.getRuntime().freeMemory()
            assertRejected("0xFFFFFFFF for type $type") { FrameCodec.decodeHeader(hdr, 0, 65535) }
            val after = Runtime.getRuntime().totalMemory() - Runtime.getRuntime().freeMemory()
            // A 4 GiB allocation cannot have happened; sanity-check we did not grow by > 1 MiB.
            assertTrue("heap grew unexpectedly for type $type", after - before < 1 shl 20)
        }
        // Also 0x80000000 (would be negative as a signed int).
        assertRejected("0x80000000") { FrameCodec.decodeHeader(hex("80 00 00 00 00 10 00 00"), 0, 65535) }
    }

    @Test
    fun ipv4_validation() {
        assertNull(FrameCodec.checkIpv4(ipv4(28), 0, 28))

        // total_length mismatch with body length.
        val mismatch = ipv4(totalLength = 40, actualLength = 28)
        assertNotNull(FrameCodec.checkIpv4(mismatch, 0, 28))
        assertRejected("total_length mismatch") { FrameCodec.validateIpv4(mismatch, 0, 28) }

        // Version nibble != 4.
        val v6 = ipv4(28).also { it[0] = 0x65 }
        assertNotNull(FrameCodec.checkIpv4(v6, 0, 28))

        // IHL < 5.
        val ihl4 = ipv4(28).also { it[0] = 0x44 }
        assertNotNull(FrameCodec.checkIpv4(ihl4, 0, 28))

        // total_length < IHL*4 (IHL=6 -> 24, total_length=22).
        val short = ipv4(22).also { it[0] = 0x46 }
        assertNotNull(FrameCodec.checkIpv4(short, 0, 22))

        // Shorter than a header at all.
        assertNotNull(FrameCodec.checkIpv4(ByteArray(19), 0, 19))

        // Validation honours the offset.
        val padded = ByteArray(8) + ipv4(28)
        assertNull(FrameCodec.checkIpv4(padded, 8, 28))
    }
}
