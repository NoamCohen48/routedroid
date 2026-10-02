package dev.routedroid.ui

import android.os.Bundle
import android.view.View
import android.widget.TextView
import androidx.activity.enableEdgeToEdge
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.isVisible
import androidx.core.view.updatePadding
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import com.google.android.material.button.MaterialButton
import dev.routedroid.R
import dev.routedroid.link
import dev.routedroid.link.LinkState
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** The launcher screen: what the link is doing, Stop, and diagnostics on request. */
class MainActivity : AppCompatActivity() {
    private lateinit var headline: TextView
    private lateinit var detail: TextView
    private lateinit var hint: TextView
    private lateinit var stop: MaterialButton
    private lateinit var diagnostics: TextView
    private lateinit var text: StatusText

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)
        text = StatusText(this)
        headline = findViewById(R.id.headline)
        detail = findViewById(R.id.detail)
        hint = findViewById(R.id.hint)
        stop = findViewById(R.id.stop)
        diagnostics = findViewById(R.id.diagnostics)
        val root = findViewById<View>(R.id.root)
        ViewCompat.setOnApplyWindowInsetsListener(root) { v, insets ->
            val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
            v.updatePadding(left = bars.left, top = bars.top, right = bars.right, bottom = bars.bottom)
            WindowInsetsCompat.CONSUMED
        }
        stop.setOnClickListener { link.stop() }
        diagnostics.isVisible = savedInstanceState?.getBoolean(KEY_DIAGNOSTICS) == true
        findViewById<MaterialButton>(R.id.toggle_diagnostics).setOnClickListener { diagnostics.isVisible = !diagnostics.isVisible }

        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                launch { link.state.collect(::render) }
                launch {
                    while (true) {
                        if (diagnostics.isVisible) renderDiagnostics()
                        delay(DIAGNOSTICS_PERIOD_MS)
                    }
                }
            }
        }
    }

    private fun render(state: LinkState) {
        headline.text = text.headline(state)
        detail.text = text.detail(state)
        detail.isVisible = detail.text.isNotEmpty()
        val h = text.hint(state)
        hint.text = h
        hint.isVisible = h != null
        stop.isEnabled = state.canStop
        renderDiagnostics()
    }

    private fun renderDiagnostics() {
        val t = link.traffic
        val config = (link.state.value as? LinkState.Active)?.config
        diagnostics.text = listOf(
            getString(R.string.diag_mtu, config?.mtu?.toString() ?: NONE),
            getString(R.string.diag_routes, config?.routes?.joinToString(" ") ?: NONE),
            getString(R.string.diag_dns, config?.dns?.joinToString(" ")?.ifEmpty { NONE } ?: NONE),
            getString(R.string.diag_sent, t.packetsOut.get(), t.bytesOut.get()),
            getString(R.string.diag_received, t.packetsIn.get(), t.bytesIn.get()),
            getString(R.string.diag_not_sent, t.droppedOut.get(), t.oversizeOut.get()),
            getString(R.string.diag_not_delivered, t.droppedIn.get()),
        ).joinToString("\n")
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putBoolean(KEY_DIAGNOSTICS, diagnostics.isVisible)
    }

    private companion object {
        const val KEY_DIAGNOSTICS = "diagnostics"
        const val DIAGNOSTICS_PERIOD_MS = 1_000L
        const val NONE = "–"
    }
}
