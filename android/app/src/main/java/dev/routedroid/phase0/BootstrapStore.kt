package dev.routedroid.phase0

import android.os.SystemClock
import android.util.Log

/**
 * The single pending bootstrap record, in process memory only. Written by [BootstrapProvider]
 * (shell UID via `adb shell content write`), consumed exactly once by [BootstrapActivity].
 * Expires after [TTL_MS]; process death loses it, which is the intended fail-closed behaviour.
 */
object BootstrapStore {
    private const val TAG = "Phase0BootstrapStore"
    const val TTL_MS = 60_000L

    private class Pending(val record: Auth.Record, val expiresAt: Long)

    private var pending: Pending? = null
    private val lock = Any()

    fun put(record: Auth.Record) {
        synchronized(lock) {
            pending?.record?.wipe()   // a new record replaces (and wipes) any older one
            pending = Pending(record, SystemClock.elapsedRealtime() + TTL_MS)
        }
        Log.i(TAG, "bootstrap record stored for session=${record.session} (expires in ${TTL_MS / 1000}s)")
    }

    /** Takes the record for [session] if present, unexpired, and matching; otherwise null. Atomic. */
    fun take(session: String): Auth.Record? {
        synchronized(lock) {
            val p = pending ?: return null.also { Log.w(TAG, "take: no pending record") }
            pending = null
            if (SystemClock.elapsedRealtime() > p.expiresAt) {
                p.record.wipe(); Log.w(TAG, "take: record expired"); return null
            }
            if (p.record.session != session) {
                p.record.wipe(); Log.w(TAG, "take: session mismatch"); return null
            }
            return p.record
        }
    }

    fun clear() = synchronized(lock) { pending?.record?.wipe(); pending = null }
}
