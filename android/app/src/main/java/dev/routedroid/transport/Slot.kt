package dev.routedroid.transport

import dev.routedroid.protocol.Protocol
import java.nio.ByteBuffer
import java.util.concurrent.ArrayBlockingQueue

/** What the socket writer sends, in queue order except for the PONG lane (§5.1). */
sealed interface TxItem {
    data object Ping : TxItem
    data object Pong : TxItem
    /** Ends the writer: it writes the session's last frame and returns. */
    data object End : TxItem
}

/**
 * One pooled frame buffer: an 8-byte header room, then up to `mtu + 1` body bytes (one
 * more than legal, so an oversize read from the VPN interface is detectable).
 */
class Slot(mtu: Int) : TxItem {
    val buf = ByteArray(Protocol.HEADER_LEN + mtu + 1)
    val buffer: ByteBuffer = ByteBuffer.wrap(buf)
    /** Bytes of [buf] in use from 0: the whole frame on tx, the bare packet on rx. */
    var len = 0
}

/** A fixed set of [Slot]s for one direction, sized by bytes rather than by count. */
class SlotPool(mtu: Int) {
    val depth = (BUDGET_BYTES / (mtu + Protocol.HEADER_LEN)).coerceIn(MIN_DEPTH, MAX_DEPTH)
    private val free = ArrayBlockingQueue<Slot>(depth).apply { repeat(depth) { add(Slot(mtu)) } }

    /** Blocks until a slot is free; interruptible. */
    fun take(): Slot = free.take()

    fun give(slot: Slot) {
        check(free.offer(slot)) { "slot returned twice" }
    }

    companion object {
        /** About 1 MiB per direction, whatever MTU the host chose. */
        const val BUDGET_BYTES = 1 shl 20
        const val MIN_DEPTH = 8
        const val MAX_DEPTH = 256
    }
}
