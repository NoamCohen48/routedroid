package dev.routedroid.protocol.frame

import dev.routedroid.protocol.Protocol
import java.io.EOFException
import java.io.InputStream

/** Blocking frame reader. Allocates the body only after the header validated. */
class FrameReader(private val input: InputStream, private val mtu: Int) {
    private val header = ByteArray(Protocol.HEADER_LEN)

    /** Returns null on a clean EOF at a frame boundary. */
    fun read(): Frame? {
        var filled = 0
        while (filled < header.size) {
            val n = input.read(header, filled, header.size - filled)
            if (n < 0) {
                if (filled == 0) return null
                throw FrameException(FrameException.TRUNCATED, "stream ended mid-header")
            }
            filled += n
        }
        val parsed = FrameHeader.parse(header)
        val type = parsed.validate(mtu)
        val body = ByteArray(parsed.bodyLength.toInt())
        var got = 0
        while (got < body.size) {
            val n = input.read(body, got, body.size - got)
            if (n < 0) throw FrameException(FrameException.TRUNCATED, "stream ended mid-body")
            got += n
        }
        return Frame(type, body)
    }
}
