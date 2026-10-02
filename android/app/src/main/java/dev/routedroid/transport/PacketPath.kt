package dev.routedroid.transport

import android.util.Log
import dev.routedroid.link.SessionEnd
import dev.routedroid.protocol.frame.FrameException
import java.io.IOException
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CountDownLatch
import java.util.concurrent.LinkedBlockingDeque
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

/**
 * The Active phase (§5 step 6): five threads around two bounded queues.
 *
 *     tun-reader → tx → socket-writer        socket-reader → rx → tun-writer
 *                            ↑ PONG lane, PING ← keepalive
 *
 * Every blocking point has exactly one waker, and [stop] fires all of them: queue waits are
 * interrupted, the TUN poll is woken, a socket read sees end-of-stream, and the writer gets
 * [TxItem.End], after which it writes the session's last frame and nothing more. A writer
 * wedged on a host that stopped reading is cut off by closing the socket. Nothing can stay
 * blocked, so [run] always returns.
 */
class PacketPath(
    internal val conn: Connection,
    internal val tun: PacketDevice,
    internal val mtu: Int,
    internal val traffic: Traffic,
    internal val clock: Clock = Clock.UPTIME,
) {
    private val end = AtomicReference<SessionEnd?>()
    private val ended = CountDownLatch(1)
    internal val txPool = SlotPool(mtu)
    internal val rxPool = SlotPool(mtu)
    /** Room for every tx slot plus PING, PONG and End, so offering never fails or blocks. */
    internal val tx = LinkedBlockingDeque<TxItem>(txPool.depth + 3)
    internal val rx = ArrayBlockingQueue<Slot>(rxPool.depth)
    internal val keepalive = Keepalive(clock.now())
    private val pongPending = AtomicBoolean(false)
    private val pingPending = AtomicBoolean(false)
    private val pumps = Pumps(this)
    private lateinit var waiters: List<Thread>
    private lateinit var writer: Thread

    /** Blocks until the session ends; returns why. The caller then closes [conn] and [tun]. */
    fun run(): SessionEnd {
        writer = pump("socket-writer") { pumps.socketWriter() }
        waiters = listOf(
            pump("tun-reader") { pumps.tunReader() },
            pump("socket-reader") { pumps.socketReader() },
            pump("tun-writer") { pumps.tunWriter() },
            pump("keepalive") { pumps.keepalive() },
        )
        (waiters + writer).forEach(Thread::start)
        ended.await()
        writer.join(FLUSH_MS)
        if (writer.isAlive) conn.close()
        for (t in waiters + writer) {
            t.join(JOIN_MS)
            if (t.isAlive) Log.w(TAG, "${t.name} did not stop")
        }
        return end.get()!!
    }

    /** Ends the path for [why] unless it already ended. Any thread, idempotent, never blocks. */
    fun stop(why: SessionEnd) {
        if (!end.compareAndSet(null, why)) return
        ended.countDown()
        tx.offerFirst(TxItem.End)
        tun.wake()
        conn.shutdownInput()
        if (::waiters.isInitialized) waiters.forEach(Thread::interrupt)
    }

    internal fun ending(): SessionEnd? = end.get()

    internal fun requestPong() {
        if (pongPending.compareAndSet(false, true)) tx.offerFirst(TxItem.Pong)
    }

    internal fun requestPing() {
        if (pingPending.compareAndSet(false, true)) tx.offer(TxItem.Ping)
    }

    internal fun sentControl(item: TxItem) {
        if (item == TxItem.Pong) pongPending.set(false) else pingPending.set(false)
    }

    /** A pump that throws ends the session; one that was stopped just returns. */
    private fun pump(name: String, body: () -> Unit) = Thread({
        try {
            body()
        } catch (_: InterruptedException) {
        } catch (e: FrameException) {
            stop(SessionEnd.Violation("bad frame: ${e.message}"))
        } catch (e: IOException) {
            stop(if (name.startsWith("socket")) SessionEnd.HostClosed else SessionEnd.Failed("VPN interface: ${e.message}"))
        } catch (e: Exception) {
            stop(SessionEnd.Failed("$name: $e"))
        }
    }, name)

    companion object {
        private const val TAG = "PacketPath"
        /** How long the writer may take to send what it is writing plus the last frame. */
        const val FLUSH_MS = 500L
        private const val JOIN_MS = 1_000L
    }
}
