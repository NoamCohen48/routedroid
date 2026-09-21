package dev.routedroid.transport

import android.os.SystemClock
import kotlinx.coroutines.delay
import java.util.concurrent.atomic.AtomicLong

/** §5.1: PING after 10 s of silence in both directions, dead after 30 s without any received frame. */
class Keepalive {
    companion object {
        const val IDLE_MS = 10_000L
        const val DEAD_MS = 30_000L
        private const val TICK_MS = 1_000L
    }

    private val lastRx = AtomicLong(now())
    private val lastTx = AtomicLong(now())
    @Volatile private var running = true

    private fun now() = SystemClock.elapsedRealtime()
    fun received() { lastRx.set(now()) }
    fun sent() { lastTx.set(now()) }
    fun stop() { running = false }

    suspend fun run(ping: () -> Boolean, dead: () -> Unit) {
        var pinged = false
        while (running) {
            delay(TICK_MS)
            val t = now()
            val sinceRx = t - lastRx.get()
            if (sinceRx >= DEAD_MS) { dead(); return }
            if (sinceRx >= IDLE_MS && t - lastTx.get() >= IDLE_MS) {
                if (!pinged) { ping(); pinged = true }
            } else {
                pinged = false
            }
        }
    }
}
