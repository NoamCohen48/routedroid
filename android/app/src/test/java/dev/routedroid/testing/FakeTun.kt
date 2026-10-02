package dev.routedroid.testing

import dev.routedroid.transport.PacketDevice
import java.io.IOException
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

/** A VPN interface in memory: [inject] is what apps send, [delivered] what the host sent them. */
class FakeTun : PacketDevice {
    private val outbound = LinkedBlockingQueue<ByteArray>()
    val delivered = LinkedBlockingQueue<ByteArray>()
    @Volatile var failWrites = false
    @Volatile var closed = false
        private set

    fun inject(packet: ByteArray) = outbound.put(packet)

    override fun read(buf: ByteArray, off: Int, len: Int): Int {
        val p = outbound.take()
        if (p === WAKE) {
            outbound.put(WAKE)
            return -1
        }
        val n = minOf(p.size, len)
        p.copyInto(buf, off, 0, n)
        return n
    }

    override fun write(buf: ByteArray, off: Int, len: Int) {
        if (failWrites) throw IOException("injected failure")
        delivered.put(buf.copyOfRange(off, off + len))
    }

    override fun wake() = outbound.put(WAKE)

    override fun close() {
        closed = true
        wake()
    }

    fun nextDelivered(): ByteArray? = delivered.poll(5, TimeUnit.SECONDS)

    private companion object {
        val WAKE = ByteArray(0)
    }
}

/** A well-formed IPv4 packet of [size] bytes (header plus zero payload). */
fun ipv4(size: Int, tag: Int = 0): ByteArray = ByteArray(size).also {
    it[0] = 0x45
    it[2] = (size ushr 8).toByte()
    it[3] = size.toByte()
    it[8] = 64
    it[9] = 17
    it[12] = 10; it[15] = 2
    it[16] = 10; it[19] = 1
    if (size > 20) it[20] = tag.toByte()
}
