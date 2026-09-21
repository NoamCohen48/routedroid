package dev.routedroid.transport

import dev.routedroid.session.StatusStore
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.joinAll
import kotlinx.coroutines.launch
import java.io.FileDescriptor

/**
 * The Active-state packet path: four pump coroutines plus keepalive, with one bounded slot
 * pool per direction, so a slow side suspends its producer instead of growing memory.
 */
class Pumps(
    private val scope: CoroutineScope,
    private val input: ChannelInput,
    private val output: ChannelOutput,
    private val tun: FileDescriptor,
    private val mtu: Int,
    /** Unblocks the socket reader; called by keepalive when the host went silent. */
    private val closeSocket: () -> Unit,
) {
    companion object {
        /** Bounded in-flight packets per direction; the reader suspends when this is full. */
        const val QUEUE_DEPTH = 256
        /** Slots must cover the queue plus one being filled and one being written. */
        const val POOL_SIZE = QUEUE_DEPTH + 2
    }

    // VPN -> socket (tx). The extra byte lets the VPN reader detect an oversize packet.
    private val txFree = Channel<Slot>(POOL_SIZE).also { c -> repeat(POOL_SIZE) { c.trySend(Slot(mtu + 1, pooled = true)) } }
    private val txFilled = Channel<Slot>(QUEUE_DEPTH)
    // socket -> VPN (rx).
    private val rxFree = Channel<Slot>(POOL_SIZE).also { c -> repeat(POOL_SIZE) { c.trySend(Slot(mtu, pooled = true)) } }
    private val rxFilled = Channel<Slot>(QUEUE_DEPTH)

    val keepalive = Keepalive()
    @Volatile private var running = true
    private var firstFailure: Throwable? = null
    private val failLock = Any()

    /** Stops the readers; the writers drain and exit when their inputs close. */
    fun stop() { running = false }

    private fun fail(t: Throwable) {
        synchronized(failLock) { if (firstFailure == null) firstFailure = t }
        running = false
        closeSocket()
    }

    /**
     * Runs until the socket reader ends (host STOP/ERROR/close, violation, keepalive death),
     * [stop] is called, or a pump fails. The first failure is rethrown after all pumps exit;
     * otherwise the reader's verdict is returned (null when stopped locally).
     */
    suspend fun run(): SocketReader.End? {
        var end: SocketReader.End? = null
        val jobs = listOf(
            scope.launch {
                try { TunReader(tun, mtu).run({ running }, txFree, txFilled) } catch (t: Throwable) { if (running) fail(t) }
                finally { txFilled.close() }
            },
            scope.launch {
                try { socketWriter() } catch (t: Throwable) { if (running) fail(t) }
            },
            scope.launch {
                try { end = SocketReader(input, mtu, keepalive).run({ running }, rxFree, rxFilled, txFilled) } catch (t: Throwable) { if (running) fail(t) }
                finally { rxFilled.close(); running = false; keepalive.stop() }
            },
            scope.launch {
                try { TunWriter(tun).run(rxFilled, rxFree) } catch (t: Throwable) { if (running) fail(t) }
            },
            scope.launch {
                keepalive.run(ping = { txFilled.trySend(Slot.PING).isSuccess }, dead = { end = SocketReader.End.Dead; fail(KeepaliveDead()) })
            },
        )
        jobs.joinAll()
        firstFailure?.let { if (it !is KeepaliveDead) throw it }
        return end
    }

    class KeepaliveDead : Exception("no frame from host for ${Keepalive.DEAD_MS / 1000}s")

    /** txFilled -> socket. Whole frame per write; returns pooled slots. */
    private suspend fun socketWriter() {
        for (slot in txFilled) {
            output.write(slot.buf, 0, slot.frameLength)
            keepalive.sent()
            if (slot.pooled) {
                StatusStore.packetsOut.incrementAndGet()
                StatusStore.bytesOut.addAndGet(slot.len.toLong())
                txFree.send(slot)
            }
        }
    }
}
