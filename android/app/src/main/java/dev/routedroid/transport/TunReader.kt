package dev.routedroid.transport

import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import android.system.StructPollfd
import dev.routedroid.protocol.Ipv4
import dev.routedroid.protocol.Protocol
import dev.routedroid.protocol.frame.MessageType
import dev.routedroid.session.StatusStore
import kotlinx.coroutines.channels.Channel
import java.io.FileDescriptor
import java.io.IOException

/** VPN fd read -> IP_PACKET frame -> txFilled. Suspends on the free pool when the writer is behind. */
class TunReader(private val tun: FileDescriptor, private val mtu: Int) {
    companion object {
        private const val POLL_TIMEOUT_MS = 500
        private val POLL_ERR = OsConstants.POLLERR or OsConstants.POLLHUP or OsConstants.POLLNVAL
    }

    private val pollfd = arrayOf(StructPollfd().apply { fd = tun; events = OsConstants.POLLIN.toShort() })

    suspend fun run(running: () -> Boolean, free: Channel<Slot>, filled: Channel<Slot>) {
        while (running()) {
            val slot = free.receive()
            val n = readPacket(running, slot) ?: run { free.trySend(slot); return }
            if (n <= 0) throw IOException("VPN fd read returned $n")
            // Oversize (interface MTU should prevent it) or non-IPv4 (stray IPv6/ND): locally
            // read, so drop rather than send an illegal frame.
            if (n > mtu || Ipv4.reject(slot.buf, Protocol.HEADER_LEN, n) != null) {
                StatusStore.droppedOut.incrementAndGet()
                free.send(slot)
                continue
            }
            slot.setHeader(MessageType.IP_PACKET, n)
            filled.send(slot)
        }
    }

    /** Polls with a timeout so a stop request is noticed; null when stopped. */
    private fun readPacket(running: () -> Boolean, slot: Slot): Int? {
        while (true) {
            if (!running()) return null
            pollfd[0].revents = 0
            val ready = try {
                Os.poll(pollfd, POLL_TIMEOUT_MS)
            } catch (e: ErrnoException) {
                if (e.errno == OsConstants.EINTR) continue else throw e
            }
            if (ready == 0) continue
            if ((pollfd[0].revents.toInt() and POLL_ERR) != 0) throw IOException("VPN fd poll error revents=${pollfd[0].revents}")
            return try {
                Os.read(tun, slot.buf, Protocol.HEADER_LEN, mtu + 1)
            } catch (e: ErrnoException) {
                if (e.errno == OsConstants.EINTR || e.errno == OsConstants.EAGAIN) continue else throw e
            }
        }
    }
}
