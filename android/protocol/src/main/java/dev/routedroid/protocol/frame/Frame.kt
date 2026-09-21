package dev.routedroid.protocol.frame

import dev.routedroid.protocol.Protocol
import java.nio.ByteBuffer

/** One frame: header + body. [encode] is byte-exact against fixtures/frames.json. */
class Frame(val type: MessageType, val body: ByteArray) {

    fun encode(): ByteArray {
        val out = ByteBuffer.allocate(Protocol.HEADER_LEN + body.size)
        out.putInt(body.size)
        out.put(Protocol.VERSION.toByte())
        out.put(type.code.toByte())
        out.putShort(0)
        out.put(body)
        return out.array()
    }

    override fun equals(other: Any?) = other is Frame && other.type == type && other.body.contentEquals(body)
    override fun hashCode() = 31 * type.hashCode() + body.contentHashCode()
    override fun toString() = "Frame(${type.wireName}, ${body.size} bytes)"

    companion object {
        fun empty(type: MessageType) = Frame(type, ByteArray(0))
        fun packet(packet: ByteArray) = Frame(MessageType.IP_PACKET, packet)

        /** Decode one frame from a buffer; returns the frame and bytes consumed. */
        fun decode(bytes: ByteArray, mtu: Int): Pair<Frame, Int> {
            if (bytes.size < Protocol.HEADER_LEN) throw FrameException(FrameException.TRUNCATED, "short header")
            val type = FrameHeader.parse(bytes).validate(mtu)
            val len = FrameHeader.parse(bytes).bodyLength.toInt()
            val end = Protocol.HEADER_LEN + len
            if (bytes.size < end) throw FrameException(FrameException.TRUNCATED, "short body")
            return Frame(type, bytes.copyOfRange(Protocol.HEADER_LEN, end)) to end
        }
    }
}
