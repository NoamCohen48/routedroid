package dev.routedroid.transport

import dev.routedroid.protocol.Protocol
import dev.routedroid.protocol.frame.MessageType
import java.nio.ByteBuffer

/** One preallocated frame buffer: header at [0, 8), body at [8, 8 + len). */
class Slot(capacity: Int, val pooled: Boolean) {
    val buf = ByteArray(Protocol.HEADER_LEN + capacity)
    var len = 0
    val frameLength: Int get() = Protocol.HEADER_LEN + len

    fun setHeader(type: MessageType, bodyLength: Int) {
        len = bodyLength
        ByteBuffer.wrap(buf, 0, Protocol.HEADER_LEN)
            .putInt(bodyLength)
            .put(Protocol.VERSION.toByte())
            .put(type.code.toByte())
            .putShort(0)
    }

    companion object {
        /** A constant empty-body frame, never returned to a pool. */
        private fun constant(type: MessageType) = Slot(0, pooled = false).apply { setHeader(type, 0) }
        val PING = constant(MessageType.PING)
        val PONG = constant(MessageType.PONG)
    }
}
