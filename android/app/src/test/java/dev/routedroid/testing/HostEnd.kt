package dev.routedroid.testing

import dev.routedroid.protocol.frame.Frame
import dev.routedroid.protocol.frame.FrameReader
import dev.routedroid.protocol.frame.MessageType
import java.io.Closeable
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket

/** The host's side of one loopback connection, speaking raw frames. */
class HostEnd(val socket: Socket) : Closeable {
    private val reader = FrameReader { dst ->
        val n = socket.getInputStream().read(dst.array(), dst.arrayOffset() + dst.position(), dst.remaining())
        if (n > 0) dst.position(dst.position() + n)
        n
    }

    init {
        socket.soTimeout = 5_000
    }

    fun send(frame: Frame) = socket.getOutputStream().write(frame.encode())

    fun send(type: MessageType, body: ByteArray = ByteArray(0)) = send(Frame(type, body))

    /** The next frame, or null at end of stream. */
    fun read(mtu: Int? = MTU): Frame? = reader.frame(mtu)

    /** Reads until [type] arrives, skipping the packet and keepalive traffic before it. */
    fun readUntil(type: MessageType): Frame? {
        while (true) {
            val f = read() ?: return null
            if (f.type == type) return f
            if (f.type != MessageType.IP_PACKET && f.type != MessageType.PONG && f.type != MessageType.PING) return f
        }
    }

    override fun close() = socket.close()

    companion object {
        const val MTU = 1400

        /** A listener on 127.0.0.1 and an unbound accept, for tests that connect a phone to it. */
        fun listen(): ServerSocket = ServerSocket(0, 1, InetAddress.getByAddress(byteArrayOf(127, 0, 0, 1)))
    }
}
