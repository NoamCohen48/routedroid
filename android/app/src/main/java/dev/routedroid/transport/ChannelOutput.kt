package dev.routedroid.transport

import dev.routedroid.protocol.frame.Frame
import java.nio.ByteBuffer
import java.nio.channels.SocketChannel

/** Blocking writes over a SocketChannel; every call writes one whole frame under a lock. */
class ChannelOutput(private val ch: SocketChannel) {
    private val lock = Any()

    fun write(buf: ByteArray, off: Int, len: Int) = synchronized(lock) {
        val bb = ByteBuffer.wrap(buf, off, len)
        while (bb.hasRemaining()) ch.write(bb)
    }

    fun writeFrame(frame: Frame) {
        val bytes = frame.encode()
        write(bytes, 0, bytes.size)
    }
}
