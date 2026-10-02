package dev.routedroid.hostile

import android.app.Activity
import android.os.Bundle
import android.util.Log
import android.widget.TextView

/**
 * Runs [Probes] off the main thread and shows the results. Each also goes to logcat as
 * `HOSTILE-RESULT <name> <PASS|FAIL|INCONCLUSIVE> <detail>`, then `HOSTILE-DONE`, which
 * is what integration-tests/emulator/hostile.sh reads.
 */
class HostileActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val view = TextView(this).apply { setPadding(32, 32, 32, 32) }
        setContentView(view)
        Thread {
            Probes(this).run { line ->
                Log.i(TAG, line)
                runOnUiThread { view.append(line + "\n") }
            }
            Log.i(TAG, "HOSTILE-DONE")
        }.start()
    }

    private companion object {
        const val TAG = "HostileProbe"
    }
}
