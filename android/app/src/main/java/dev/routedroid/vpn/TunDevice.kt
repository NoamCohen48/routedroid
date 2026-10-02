package dev.routedroid.vpn

import android.os.ParcelFileDescriptor
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import android.system.StructPollfd
import dev.routedroid.transport.PacketDevice
import java.io.IOException
import java.util.concurrent.atomic.AtomicBoolean

/**
 * The VPN interface fd. A read polls it together with a self-pipe, so [wake] ends a read
 * that closing the fd from another thread would not (and closing under a reader would risk
 * the fd number being reused).
 */
class TunDevice(private val pfd: ParcelFileDescriptor) : PacketDevice {
    private val fd = pfd.fileDescriptor
    private val wakeFds = Os.pipe()
    private val poll = arrayOf(
        StructPollfd().apply { this.fd = this@TunDevice.fd; events = OsConstants.POLLIN.toShort() },
        StructPollfd().apply { this.fd = wakeFds[0]; events = OsConstants.POLLIN.toShort() },
    )
    private val woken = AtomicBoolean(false)
    private val closed = AtomicBoolean(false)

    override fun read(buf: ByteArray, off: Int, len: Int): Int {
        while (true) {
            poll.forEach { it.revents = 0 }
            try {
                Os.poll(poll, -1)
                if (poll[1].revents.toInt() != 0) return -1
                val revents = poll[0].revents.toInt()
                if (revents and (OsConstants.POLLERR or OsConstants.POLLHUP or OsConstants.POLLNVAL) != 0) {
                    throw IOException("poll revents 0x${revents.toString(16)}")
                }
                if (revents and OsConstants.POLLIN != 0) return Os.read(fd, buf, off, len)
            } catch (e: ErrnoException) {
                if (e.errno != OsConstants.EINTR && e.errno != OsConstants.EAGAIN) throw IOException(e)
            }
        }
    }

    override fun write(buf: ByteArray, off: Int, len: Int) {
        while (true) {
            try {
                val n = Os.write(fd, buf, off, len)
                if (n != len) throw IOException("short write: $n of $len bytes")
                return
            } catch (e: ErrnoException) {
                // EINTR means nothing was transferred, so retrying cannot duplicate a packet.
                if (e.errno != OsConstants.EINTR) throw IOException(e)
            }
        }
    }

    override fun wake() {
        if (!woken.compareAndSet(false, true)) return
        try { Os.write(wakeFds[1], byteArrayOf(1), 0, 1) } catch (_: ErrnoException) {}
    }

    override fun close() {
        if (!closed.compareAndSet(false, true)) return
        wake()
        try { pfd.close() } catch (_: IOException) {}
        for (f in wakeFds) try { Os.close(f) } catch (_: ErrnoException) {}
    }
}
