package dev.routedroid.bootstrap

import dev.routedroid.protocol.auth.BootstrapRecord
import dev.routedroid.transport.Clock
import java.util.concurrent.ScheduledExecutorService
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit

/**
 * The one pending bootstrap record (§7.1), in memory only. A new record replaces and wipes
 * the pending one; an unclaimed record is wiped [ttlMs] after it arrived, by a timer rather
 * than on the next look. Process death loses it, which fails closed.
 */
class RecordVault(
    private val clock: Clock,
    private val timers: ScheduledExecutorService,
    private val ttlMs: Long = TTL_MS,
) {
    private val lock = Any()
    private var pending: BootstrapRecord? = null
    private var expiresAt = 0L
    private var wipe: ScheduledFuture<*>? = null

    fun put(record: BootstrapRecord) = synchronized(lock) {
        discard()
        pending = record
        expiresAt = clock.now() + ttlMs
        wipe = timers.schedule({ expire(record) }, ttlMs, TimeUnit.MILLISECONDS)
    }

    /**
     * The record, if one is pending, unexpired and for [session]; the caller then owns it.
     * A launch naming another session leaves the record where it is: any app can start the
     * activity, and a wrong guess must not cost the host its record.
     */
    fun take(session: String): BootstrapRecord? = synchronized(lock) {
        val record = pending ?: return null
        if (clock.now() >= expiresAt) {
            discard()
            return null
        }
        if (record.session != session) return null
        wipe?.cancel(false)
        pending = null
        record
    }

    private fun expire(record: BootstrapRecord) = synchronized(lock) {
        if (pending === record) discard()
    }

    private fun discard() {
        wipe?.cancel(false)
        pending?.wipe()
        pending = null
    }

    companion object {
        const val TTL_MS = 60_000L
    }
}
