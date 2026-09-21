package dev.routedroid.transport

import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import dev.routedroid.protocol.Protocol
import dev.routedroid.session.StatusStore
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.channels.ClosedReceiveChannelException
import java.io.FileDescriptor
import java.io.IOException

/** rxFilled -> VPN fd. One whole packet per write; a short write is fatal; EINTR retries. */
class TunWriter(private val tun: FileDescriptor) {
    suspend fun run(filled: Channel<Slot>, free: Channel<Slot>) {
        try {
            for (slot in filled) {
                val written = writeRetrying(slot)
                if (written != slot.len) throw IOException("short VPN write: $written of ${slot.len} bytes")
                StatusStore.packetsIn.incrementAndGet()
                StatusStore.bytesIn.addAndGet(slot.len.toLong())
                free.send(slot)
            }
        } catch (_: ClosedReceiveChannelException) {
            // rxFilled closed by the socket reader: normal end.
        }
    }

    private fun writeRetrying(slot: Slot): Int {
        while (true) {
            return try {
                Os.write(tun, slot.buf, Protocol.HEADER_LEN, slot.len)
            } catch (e: ErrnoException) {
                // EINTR means the kernel transferred nothing; retrying is safe.
                if (e.errno == OsConstants.EINTR) continue else throw e
            }
        }
    }
}
