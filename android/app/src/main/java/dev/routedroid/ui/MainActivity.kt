package dev.routedroid.ui

import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.widget.Button
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import dev.routedroid.R
import dev.routedroid.session.StatusStore
import dev.routedroid.vpn.RoutedroidVpnService

/** Launcher UI: status text fed from [StatusStore] plus a Stop button. */
class MainActivity : AppCompatActivity() {
    private lateinit var statusView: TextView
    private val handler = Handler(Looper.getMainLooper())
    // Counters are plain atomics (no per-packet StateFlow emission), so the UI just polls.
    // Only touch the view when the text changed, so an idle session leaves the UI idle.
    private val tick = object : Runnable {
        override fun run() {
            val text = StatusText.render(StatusStore.status.value)
            if (statusView.text.toString() != text) statusView.text = text
            handler.postDelayed(this, 500)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)
        statusView = findViewById(R.id.status)
        findViewById<Button>(R.id.stop).setOnClickListener {
            startService(Intent(this, RoutedroidVpnService::class.java).setAction(RoutedroidVpnService.ACTION_STOP))
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
}
