package dev.routedroid.transport

import dev.routedroid.protocol.frame.Frame
import dev.routedroid.protocol.frame.FrameReader
import java.io.IOException
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.Socket
import java.net.SocketTimeoutException
import java.nio.ByteBuffer

/**
 * The socket to the host's adb-reverse port and the one [FrameReader] over it, from connect
 * to close. Reads honour the deadline set by [within]; nothing else ever times out, because
 * [shutdownInput] and [close] are how other threads end a blocked read.
 */
class Connection(private val clock: Clock = Clock.UPTIME) {
    val socket = Socket()
    private val writeLock = Any()
    @Volatile private var deadline = NO_DEADLINE

    val reader = FrameReader { dst -> read(dst) }

    fun connect(port: Int, timeoutMs: Int) {
        // 127.0.0.1, not getLoopbackAddress(): adbd's reverse listener is IPv4 only.
        socket.connect(InetSocketAddress(InetAddress.getByAddress(byteArrayOf(127, 0, 0, 1)), port), timeoutMs)
        socket.tcpNoDelay = true
    }

    /** Runs [block] with every read inside it bounded by one deadline [ms] from now. */
    fun <T> within(ms: Long, block: () -> T): T {
        deadline = clock.now() + ms
        try {
            return block()
        } finally {
            deadline = NO_DEADLINE
            if (!socket.isClosed) runCatching { socket.soTimeout = 0 }
        }
    }

    private fun read(dst: ByteBuffer): Int {
        val end = deadline
        if (end != NO_DEADLINE) {
            val left = end - clock.now()
            if (left <= 0) throw SocketTimeoutException("deadline passed")
            socket.soTimeout = left.coerceAtMost(Int.MAX_VALUE.toLong()).toInt()
        }
        val n = socket.getInputStream().read(dst.array(), dst.arrayOffset() + dst.position(), dst.remaining())
        if (n > 0) dst.position(dst.position() + n)
        return n
    }

    /** Bytes already received and not yet read: a frame the host sent while the app was busy. */
    fun pending(): Int = socket.getInputStream().available()

    fun send(frame: Frame) {
        val bytes = frame.encode()
        write(bytes, 0, bytes.size)
    }

    /** One whole frame per call; frames from different threads never interleave. */
    fun write(buf: ByteArray, off: Int, len: Int) {
        synchronized(writeLock) { socket.getOutputStream().write(buf, off, len) }
    }

    /** Ends any blocked read with end-of-stream; writing still works. Any thread. */
    fun shutdownInput() {
        try { socket.shutdownInput() } catch (_: IOException) {}
    }

    /** Ends everything, including a write blocked on a host that stopped reading. Any thread. */
    fun close() {
        try { socket.close() } catch (_: IOException) {}
    }

    private companion object {
        const val NO_DEADLINE = Long.MIN_VALUE
    }
}
