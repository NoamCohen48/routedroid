package dev.routedroid.protocol.frame

import dev.routedroid.protocol.Protocol
import java.nio.ByteBuffer

/**
 * The 8-byte header (§2). [validate] runs before any body buffer exists, so a
 * hostile body length never allocates.
 */
data class FrameHeader(val bodyLength: Long, val version: Int, val messageType: Int, val flags: Int) {

    fun validate(mtu: Int): MessageType {
        if (version != Protocol.VERSION) throw FrameException(FrameException.UNSUPPORTED_VERSION, "version $version")
        if (flags != 0) throw FrameException(FrameException.NONZERO_FLAGS, "flags 0x${flags.toString(16)}")
        val type = MessageType.fromCode(messageType)
            ?: throw FrameException(FrameException.UNKNOWN_TYPE, "type 0x${messageType.toString(16)}")
        val len = bodyLength
        when {
            type == MessageType.IP_PACKET -> {
                val max = minOf(mtu, Protocol.MAX_PACKET_BODY)
                if (len < Protocol.MIN_PACKET_BODY || len > max)
                    throw FrameException(FrameException.PACKET_BODY_OUT_OF_RANGE, "packet body $len outside [${Protocol.MIN_PACKET_BODY}, $max]")
            }
            type.emptyBody -> if (len != 0L)
                throw FrameException(FrameException.UNEXPECTED_BODY, "${type.wireName} must be empty, got $len")
            else -> {
                if (len == 0L) throw FrameException(FrameException.EMPTY_BODY, "${type.wireName} must carry a body")
                if (len > Protocol.MAX_CONTROL_BODY)
                    throw FrameException(FrameException.CONTROL_BODY_TOO_LARGE, "${type.wireName} body $len over ${Protocol.MAX_CONTROL_BODY}")
            }
        }
        return type
    }

    companion object {
        fun parse(bytes: ByteArray, offset: Int = 0): FrameHeader {
            val b = ByteBuffer.wrap(bytes, offset, Protocol.HEADER_LEN)
            return FrameHeader(
                bodyLength = b.int.toLong() and 0xFFFF_FFFFL,
                version = b.get().toInt() and 0xFF,
                messageType = b.get().toInt() and 0xFF,
                flags = b.short.toInt() and 0xFFFF,
            )
        }
    }
}
