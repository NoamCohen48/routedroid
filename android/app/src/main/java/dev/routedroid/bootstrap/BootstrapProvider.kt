package dev.routedroid.bootstrap

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.net.Uri
import android.os.Binder
import android.os.ParcelFileDescriptor
import android.util.Log
import dev.routedroid.protocol.auth.BootstrapRecord
import java.io.FileInputStream
import java.io.IOException

/**
 * Receives the §7.1 bootstrap record on the write side of a socket pair:
 *
 * ```
 * adb shell content write --uri content://dev.routedroid.bootstrap/record < record
 * ```
 *
 * Exported but guarded twice: the manifest requires `android.permission.DUMP` (only
 * shell/system hold it) and [openFile] additionally requires the Binder caller to be the
 * shell UID. Everything else is denied. Nothing is persisted; see [BootstrapStore].
 */
class BootstrapProvider : ContentProvider() {
    companion object {
        private const val TAG = "BootstrapProvider"
        const val AUTHORITY = "dev.routedroid.bootstrap"
        const val PATH_RECORD = "record"
        /** android.os.Process.SHELL_UID (hidden API constant). */
        private const val SHELL_UID = 2000
        private const val READ_TIMEOUT_MS = 5_000L
    }

    override fun onCreate(): Boolean = true

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
        val uid = Binder.getCallingUid()
        if (uid != SHELL_UID) {
            Log.w(TAG, "openFile denied: caller uid=$uid is not shell")
            throw SecurityException("bootstrap record must come from the adb shell")
        }
        if (uri.authority != AUTHORITY || uri.path != "/$PATH_RECORD") throw IllegalArgumentException("unknown uri")
        if (mode != "w" && mode != "wt") throw SecurityException("write-only")
        // A socketpair, NOT a pipe: `content write` copies stdin with FileUtils.copy(), which
        // takes a splice() fast path whenever either side is a FIFO. adb's stdin is an AF_UNIX
        // socket, and on older kernels (Samsung Android 10, 3.18) splice from it fails with
        // EINVAL and `content` gives up without a userspace fallback. With neither side a FIFO
        // or regular file, copy() falls back to a plain read/write loop everywhere.
        val pair = ParcelFileDescriptor.createReliableSocketPair()
        val readEnd = pair[0]
        Thread({ drain(readEnd) }, "bootstrap-record-reader").start()
        return pair[1]
    }

    /** Reads exactly one record then EOF; anything else is discarded. */
    private fun drain(readEnd: ParcelFileDescriptor) {
        val buf = ByteArray(BootstrapRecord.LENGTH + 1)
        var n = 0
        // Bounded wait: a stalled writer must not keep this thread (and a half record) forever.
        val killer = Thread {
            try { Thread.sleep(READ_TIMEOUT_MS) } catch (_: InterruptedException) { return@Thread }
            try { readEnd.closeWithError("timeout") } catch (_: IOException) {}
        }.apply { isDaemon = true; start() }
        try {
            FileInputStream(readEnd.fileDescriptor).use { input ->
                while (n < buf.size) {
                    val r = input.read(buf, n, buf.size - n)
                    if (r < 0) break
                    n += r
                }
            }
        } catch (e: IOException) {
            Log.w(TAG, "record read failed: ${e.message}")
            n = -1
        } finally {
            killer.interrupt()
            try { readEnd.close() } catch (_: IOException) {}
        }
        if (n != BootstrapRecord.LENGTH) {
            Log.w(TAG, "record rejected: got $n bytes, want ${BootstrapRecord.LENGTH}")
            buf.fill(0); return
        }
        val record = BootstrapRecord.decode(buf.copyOf(BootstrapRecord.LENGTH))
        buf.fill(0)
        if (record == null) { Log.w(TAG, "record rejected: bad layout"); return }
        BootstrapStore.put(record)
    }

    override fun query(uri: Uri, projection: Array<String>?, selection: String?, selectionArgs: Array<String>?, sortOrder: String?): Cursor? =
        throw SecurityException("write-only")
    override fun getType(uri: Uri): String? = null
    override fun insert(uri: Uri, values: ContentValues?): Uri? = throw SecurityException("write-only")
    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<String>?): Int = throw SecurityException("write-only")
    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<String>?): Int = throw SecurityException("write-only")
}
