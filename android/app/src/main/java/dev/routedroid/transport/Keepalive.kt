package dev.routedroid.transport

import java.util.concurrent.atomic.AtomicLong

/**
 * §5.1 as pure arithmetic over [Clock] readings: PING after [IDLE_MS] with no frame either
 * way, dead after [DEAD_MS] with no frame received. The packet path calls [check] at the
 * times [nextCheckIn] names, so an idle session wakes about every 10 s, not every second.
 */
class Keepalive(now: Long) {
    enum class Verdict { NONE, PING, DEAD }

    private val lastRx = AtomicLong(now)
    private val lastTx = AtomicLong(now)

    fun received(now: Long) = lastRx.set(now)

    fun sent(now: Long) = lastTx.set(now)

    fun check(now: Long): Verdict {
        val rx = lastRx.get()
        return when {
            now - rx >= DEAD_MS -> Verdict.DEAD
            now - rx >= IDLE_MS && now - lastTx.get() >= IDLE_MS -> Verdict.PING
            else -> Verdict.NONE
        }
    }

    /**
     * Milliseconds until [check] could answer differently. Once idle, the PING just asked
     * for may not be written yet, so the next one is due an idle interval from now.
     */
    fun nextCheckIn(now: Long): Long {
        val rx = lastRx.get()
        val idleAt = maxOf(rx, lastTx.get()) + IDLE_MS
        val pingAt = if (idleAt > now) idleAt else now + IDLE_MS
        return (minOf(pingAt, rx + DEAD_MS) - now).coerceAtLeast(MIN_WAIT_MS)
    }

    companion object {
        const val IDLE_MS = 10_000L
        const val DEAD_MS = 30_000L
        private const val MIN_WAIT_MS = 10L
    }
}
