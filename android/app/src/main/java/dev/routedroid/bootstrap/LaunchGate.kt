package dev.routedroid.bootstrap

import android.os.SystemClock

/**
 * Rate limit on bootstrap launches. Anyone on the device can `am start` the exported
 * activity; a record is consumed per attempt regardless, but this also keeps a hostile
 * app from spinning the activity (and its host connect) in a tight loop.
 */
object LaunchGate {
    private const val WINDOW_MS = 10_000L
    private const val MAX_IN_WINDOW = 3

    private val stamps = ArrayDeque<Long>()
    private val lock = Any()

    /** Returns true if this launch may proceed. */
    fun tryAcquire(now: Long = SystemClock.elapsedRealtime()): Boolean = synchronized(lock) {
        while (stamps.isNotEmpty() && now - stamps.first() > WINDOW_MS) stamps.removeFirst()
        if (stamps.size >= MAX_IN_WINDOW) return false
        stamps.addLast(now)
        true
    }
}
