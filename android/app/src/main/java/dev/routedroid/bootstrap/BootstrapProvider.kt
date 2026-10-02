package dev.routedroid.bootstrap

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.net.Uri
import android.os.Binder
import android.os.ParcelFileDescriptor
import android.os.SystemClock
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import android.system.StructPollfd
import android.util.Log
import dev.routedroid.link
import dev.routedroid.protocol.auth.BootstrapRecord
import java.io.FileDescriptor
import java.io.IOException

/**
 * Receives the §7.1 bootstrap record:
 *
 *     adb shell content write --uri content://dev.routedroid.bootstrap/record < record
 *
 * Exported but guarded twice: the manifest requires `android.permission.DUMP`, which only
 * the shell and the system hold, and [openFile] requires the shell UID itself.
 */
class BootstrapProvider : ContentProvider() {
    override fun onCreate(): Boolean = true

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
        if (Binder.getCallingUid() != SHELL_UID) throw SecurityException("the bootstrap record comes from the adb shell")
        if (uri.toString() != BootstrapRecord.PROVIDER_URI) throw IllegalArgumentException("unknown uri")
        if (mode != "w" && mode != "wt") throw SecurityException("write-only")
        // A socket pair, not a pipe: `content write` copies with FileUtils.copy(), whose
        // splice() fast path for FIFOs fails with EINVAL on old kernels (Samsung Android 10)
        // when its stdin is adb's AF_UNIX socket.
        val (readEnd, writeEnd) = ParcelFileDescriptor.createReliableSocketPair()
        Os.shutdown(writeEnd.fileDescriptor, OsConstants.SHUT_RD)
        Thread({ drain(readEnd) }, "bootstrap-record").start()
        return writeEnd
    }

    /** Exactly one record, then end of stream, within [READ_TIMEOUT_MS]; anything else is dropped. */
    private fun drain(readEnd: ParcelFileDescriptor) {
        val buf = ByteArray(BootstrapRecord.LENGTH + 1)
        try {
            val n = readAll(readEnd.fileDescriptor, buf)
            val record = if (n == BootstrapRecord.LENGTH) buf.copyOf(n).let { exact -> BootstrapRecord.decode(exact).also { exact.fill(0) } } else null
            if (record == null) return reject(readEnd, "not a version-1 record ($n bytes)")
            context!!.link.vault.put(record)
            readEnd.close()
        } catch (e: IOException) {
            reject(readEnd, e.message ?: "read failed")
        } finally {
            buf.fill(0)
        }
    }

    /** Polls before every read, so a writer that stalls cannot hold this thread past the deadline. */
    private fun readAll(fd: FileDescriptor, buf: ByteArray): Int {
        val deadline = SystemClock.uptimeMillis() + READ_TIMEOUT_MS
        val poll = arrayOf(StructPollfd().apply { this.fd = fd; events = OsConstants.POLLIN.toShort() })
        var n = 0
        while (n < buf.size) {
            val left = deadline - SystemClock.uptimeMillis()
            if (left <= 0) throw IOException("timed out")
            try {
                if (Os.poll(poll, left.toInt()) == 0) continue
                val r = Os.read(fd, buf, n, buf.size - n)
                if (r == 0) break
                n += r
            } catch (e: ErrnoException) {
                if (e.errno != OsConstants.EINTR) throw IOException(e)
            }
        }
        return n
    }

    /** Tells the writing side, and the log, without a word about the content. */
    private fun reject(readEnd: ParcelFileDescriptor, why: String) {
        Log.w(TAG, "bootstrap record rejected: $why")
        try { readEnd.closeWithError(why) } catch (_: IOException) {}
    }

    override fun query(uri: Uri, projection: Array<String>?, selection: String?, selectionArgs: Array<String>?, sortOrder: String?): Cursor =
        throw SecurityException("write-only")
    override fun getType(uri: Uri): String? = null
    override fun insert(uri: Uri, values: ContentValues?): Uri = throw SecurityException("write-only")
    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<String>?): Int = throw SecurityException("write-only")
    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<String>?): Int =
        throw SecurityException("write-only")

    private companion object {
        const val TAG = "BootstrapProvider"
        /** Process.SHELL_UID, public only from API 29; the value is fixed. */
        const val SHELL_UID = 2000
        const val READ_TIMEOUT_MS = 5_000L
    }
}
