package dev.routedroid.transport

import dev.routedroid.protocol.Protocol
import dev.routedroid.protocol.frame.Frame
import dev.routedroid.protocol.frame.FrameException
import dev.routedroid.protocol.frame.FrameHeader
import dev.routedroid.protocol.frame.MessageType
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.channels.SocketChannel

/**
 * Blocking reads over a SocketChannel.
 *
 * Direct channel I/O, NOT `ch.socket().getInputStream()`: the SocketAdaptor streams
 * synchronize on `blockingLock()` for the whole blocking call, so a blocked reader starves
 * the writer (deadlock observed on Android 14). SocketChannelImpl has independent locks.
 */
class ChannelInput(private val ch: SocketChannel) {
    private val header = ByteArray(Protocol.HEADER_LEN)

    fun readFully(buf: ByteArray, off: Int = 0, len: Int = buf.size) {
        val bb = ByteBuffer.wrap(buf, off, len)
        while (bb.hasRemaining()) {
            if (ch.read(bb) < 0) throw IOException("peer closed connection")
        }
    }

    /**
     * Reads and validates one header. Null on a clean EOF at a frame boundary. The body
     * length is bounded by [FrameHeader.validate] before any buffer is allocated.
     */
    fun readHeader(mtu: Int): Pair<MessageType, Int>? {
        val bb = ByteBuffer.wrap(header)
        while (bb.hasRemaining()) {
            if (ch.read(bb) < 0) {
                if (bb.position() == 0) return null
                throw FrameException(FrameException.TRUNCATED, "stream ended mid-header")
            }
        }
        val parsed = FrameHeader.parse(header)
        return parsed.validate(mtu) to parsed.bodyLength.toInt()
    }

    /** Reads a whole frame, allocating its body. For control frames only. */
    fun readFrame(mtu: Int): Frame? {
        val (type, len) = readHeader(mtu) ?: return null
        val body = ByteArray(len)
        readFully(body)
        return Frame(type, body)
    }
}
