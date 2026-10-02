package dev.routedroid.protocol.frame

import dev.routedroid.protocol.Protocol
import java.nio.ByteBuffer

/** The 8-byte header (§2): `body_length u32 | version u8 | type u8 | flags u16`, big-endian. */
object FrameHeader {
    /** Writes a header at [at] in [buf]; no allocation, so the packet path can use it. */
    fun write(buf: ByteArray, at: Int, type: MessageType, bodyLength: Int) {
        buf[at] = (bodyLength ushr 24).toByte()
        buf[at + 1] = (bodyLength ushr 16).toByte()
        buf[at + 2] = (bodyLength ushr 8).toByte()
        buf[at + 3] = bodyLength.toByte()
        buf[at + 4] = Protocol.VERSION.toByte()
        buf[at + 5] = type.code.toByte()
        buf[at + 6] = 0
        buf[at + 7] = 0
    }

    /**
     * Checks a complete header (absolute reads at 0–7 of [h]) before any of its body is read
     * or allocated, so a hostile length never costs memory. [mtu] is the negotiated value, or
     * null before HELLO_ACK, when IP_PACKET is illegal anyway (§5) and only the absolute
     * limit applies.
     */
    fun validate(h: ByteBuffer, mtu: Int?): MessageType {
        val length = h.getInt(0).toLong() and 0xFFFF_FFFFL
        val version = h.get(4).toInt() and 0xFF
        val code = h.get(5).toInt() and 0xFF
        val flags = h.getShort(6).toInt() and 0xFFFF
        if (version != Protocol.VERSION) throw FrameException(FrameException.UNSUPPORTED_VERSION, "version $version")
        if (flags != 0) throw FrameException(FrameException.NONZERO_FLAGS, "flags 0x${flags.toString(16)}")
        val type = MessageType.fromCode(code)
            ?: throw FrameException(FrameException.UNKNOWN_TYPE, "type 0x${code.toString(16)}")
        when {
            type == MessageType.IP_PACKET -> {
                val max = minOf(mtu ?: Protocol.MAX_PACKET_BODY, Protocol.MAX_PACKET_BODY)
                if (length < Protocol.MIN_PACKET_BODY || length > max) throw FrameException(
                    FrameException.PACKET_BODY_OUT_OF_RANGE, "packet body $length outside [${Protocol.MIN_PACKET_BODY}, $max]",
                )
            }
            type.emptyBody -> if (length != 0L)
                throw FrameException(FrameException.UNEXPECTED_BODY, "$type must be empty, got $length")
            length == 0L -> throw FrameException(FrameException.EMPTY_BODY, "$type must carry a body")
            length > Protocol.MAX_CONTROL_BODY -> throw FrameException(
                FrameException.CONTROL_BODY_TOO_LARGE, "$type body $length over ${Protocol.MAX_CONTROL_BODY}",
            )
        }
        return type
    }
}
