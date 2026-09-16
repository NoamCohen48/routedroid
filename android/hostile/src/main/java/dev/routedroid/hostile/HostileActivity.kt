package dev.routedroid.hostile

import android.app.Activity
import android.content.ComponentName
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.util.Log
import android.widget.TextView

/**
 * Phase 0 §3.4 hostile-app probe. On launch it attempts what a malicious installed app could:
 *  1. write a forged bootstrap record into the provider;
 *  2. read from the provider;
 *  3. launch BootstrapActivity with plausible extras (expecting no VPN prompt/service).
 * Results go to logcat as `HOSTILE-RESULT <n> <PASS|FAIL> <detail>` (PASS = denied).
 */
class HostileActivity : Activity() {
    companion object { private const val TAG = "HostileProbe" }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val uri = Uri.parse("content://dev.routedroid.phase0.bootstrap/record")
        val lines = ArrayList<String>()
        fun result(n: Int, denied: Boolean, detail: String) {
            val line = "HOSTILE-RESULT $n ${if (denied) "PASS" else "FAIL"} $detail"
            Log.i(TAG, line); lines += line
        }
        // 1. forged record write
        try {
            contentResolver.openOutputStream(uri, "w").use { out ->
                val rec = ByteArray(80); "RDB0".toByteArray().copyInto(rec, 0)
                intent.getStringExtra("session").orEmpty().toByteArray().copyInto(rec, 8)
                out!!.write(rec)
            }
            result(1, false, "provider accepted a forged record")
        } catch (e: SecurityException) {
            result(1, true, "write denied: ${e.message?.take(80)}")
        } catch (e: Exception) {
            result(1, e is SecurityException, "write threw ${e.javaClass.simpleName}: ${e.message?.take(80)}")
        }
        // 2. read
        try {
            contentResolver.openInputStream(uri).use { it!!.read() }
            result(2, false, "provider allowed read")
        } catch (e: SecurityException) {
            result(2, true, "read denied: ${e.message?.take(80)}")
        } catch (e: Exception) {
            result(2, e is SecurityException, "read threw ${e.javaClass.simpleName}: ${e.message?.take(80)}")
        }
        // 3. launch the exported activity with guessed/observed extras
        try {
            startActivity(Intent().setComponent(ComponentName("dev.routedroid.phase0", "dev.routedroid.phase0.BootstrapActivity"))
                .putExtra("session", intent.getStringExtra("session") ?: "guess")
                .putExtra("device_port", intent.getIntExtra("device_port", 9000)))
            result(3, true, "launched BootstrapActivity; check that no VPN prompt/service appeared")
        } catch (e: Exception) {
            result(3, true, "launch threw ${e.javaClass.simpleName}")
        }
        setContentView(TextView(this).apply { text = lines.joinToString("\n"); setPadding(32, 32, 32, 32) })
    }
}
