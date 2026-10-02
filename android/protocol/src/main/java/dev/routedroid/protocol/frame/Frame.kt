package dev.routedroid.protocol.frame

import dev.routedroid.protocol.Protocol

/** One whole frame, for control messages. The packet path writes headers in place instead. */
class Frame(val type: MessageType, val body: ByteArray = ByteArray(0)) {
    /** Byte-exact against fixtures/frames.json. */
    fun encode(): ByteArray {
        val out = ByteArray(Protocol.HEADER_LEN + body.size)
        FrameHeader.write(out, 0, type, body.size)
        body.copyInto(out, Protocol.HEADER_LEN)
        return out
    }

    override fun toString() = "Frame($type, ${body.size} bytes)"
}
