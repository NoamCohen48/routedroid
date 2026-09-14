package dev.routedroid.phase0

import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.widget.Button
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity

/** Launcher UI: status text fed from [StatusStore] plus a Stop button. */
class MainActivity : AppCompatActivity() {

    private lateinit var statusView: TextView
    private val handler = Handler(Looper.getMainLooper())
    // Counters are plain atomics (no per-packet StateFlow emission), so the UI just polls.
    private val tick = object : Runnable {
        override fun run() {
            render(StatusStore.status.value)
            handler.postDelayed(this, 500)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)
        statusView = findViewById(R.id.status)
        findViewById<Button>(R.id.stop).setOnClickListener {
            startService(Intent(this, Phase0VpnService::class.java).setAction(Phase0VpnService.ACTION_STOP))
        }
    }

    override fun onStart() {
        super.onStart()
        handler.post(tick)
    }

    override fun onStop() {
        handler.removeCallbacks(tick)
        super.onStop()
    }

    private fun render(s: StatusStore.Status) {
        statusView.text = buildString {
            appendLine("state:      ${s.state}")
            appendLine("session:    ${s.session ?: "-"}")
            appendLine("port:       ${s.devicePort ?: "-"}")
            appendLine("mtu:        ${s.mtu ?: "-"}")
            appendLine("addresses:  ${s.addresses.ifEmpty { listOf("-") }.joinToString(" ")}")
            appendLine("routes:     ${s.routes.ifEmpty { listOf("-") }.joinToString(" ")}")
            appendLine("dns:        ${s.dns.ifEmpty { listOf("-") }.joinToString(" ")}")
            appendLine("pkts out:   ${StatusStore.packetsOut.get()} (${StatusStore.bytesOut.get()} B)")
            appendLine("pkts in:    ${StatusStore.packetsIn.get()} (${StatusStore.bytesIn.get()} B)")
            appendLine("dropped:    ${StatusStore.droppedOut.get()}")
            appendLine("last error: ${s.lastError ?: "-"}")
        }
    }
}
