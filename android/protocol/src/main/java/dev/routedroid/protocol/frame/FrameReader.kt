package dev.routedroid.protocol.frame

import dev.routedroid.protocol.Protocol
import java.nio.ByteBuffer

/** Where a [FrameReader] reads from: a blocking read into [dst], -1 at the end of the stream. */
fun interface ByteSource {
    fun read(dst: ByteBuffer): Int
}

/**
 * The one frame reader, used on the phone and against the fixtures. [next] reads and checks
 * a header (§2) before anything of its body; the caller then reads the body with [readBody]
 * into a buffer it owns (the packet path, which allocates nothing per frame) or [body].
 */
class FrameReader(private val source: ByteSource) {
    private val header = ByteBuffer.allocate(Protocol.HEADER_LEN)

    /** Of the header [next] last returned true for. */
    var type: MessageType = MessageType.STOP
        private set
    var bodyLength: Int = 0
        private set

    /** False on a clean end of stream at a frame boundary; [mtu] is null before HELLO_ACK. */
    fun next(mtu: Int?): Boolean {
        header.clear()
        while (header.hasRemaining()) {
            if (source.read(header) < 0) {
                if (header.position() == 0) return false
                throw FrameException(FrameException.TRUNCATED, "stream ended mid-header")
            }
        }
        type = FrameHeader.validate(header, mtu)
        bodyLength = header.getInt(0)
        return true
    }

    /** Fills [dst] from its position with exactly [bodyLength] bytes. */
    fun readBody(dst: ByteBuffer) {
        require(dst.remaining() >= bodyLength) { "buffer too small for the body" }
        val end = dst.position() + bodyLength
        val limit = dst.limit()
        dst.limit(end)
        try {
            while (dst.hasRemaining()) {
                if (source.read(dst) < 0) throw FrameException(FrameException.TRUNCATED, "stream ended mid-body")
            }
        } finally {
            dst.limit(limit)
        }
    }

    /** The body as a new array; for control frames, which are rare and small. */
    fun body(): ByteArray = ByteArray(bodyLength).also { readBody(ByteBuffer.wrap(it)) }

    /** The next whole frame, or null on a clean end of stream. */
    fun frame(mtu: Int?): Frame? = if (next(mtu)) Frame(type, body()) else null
}
