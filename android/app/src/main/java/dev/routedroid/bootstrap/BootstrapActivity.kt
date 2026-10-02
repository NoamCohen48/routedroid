package dev.routedroid.bootstrap

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.widget.TextView
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import dev.routedroid.R
import dev.routedroid.link
import dev.routedroid.link.LinkState
import dev.routedroid.ui.MainActivity
import dev.routedroid.ui.isFailure
import kotlinx.coroutines.launch

/**
 * §7.2 entry point, started by the host after it wrote the bootstrap record:
 *
 *     adb shell am start -n dev.routedroid/.bootstrap.BootstrapActivity --es session <id>
 *
 * Launch input is untrusted: the session only selects the pending record, and a launch that
 * matches none finishes before any content exists, so its translucent window never shows.
 * Everything else lives in [DeviceLink][dev.routedroid.link.DeviceLink]; this screen
 * survives recreation by re-rendering the link's state and only asks for consent.
 */
class BootstrapActivity : AppCompatActivity() {
    private val consent = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        link.consent(it.resultCode == RESULT_OK)
    }
    private val notifications = registerForActivityResult(ActivityResultContracts.RequestPermission()) { askConsent() }
    /** Consent was requested for the current session; the system dialog may still be up. */
    private var asked = false
    /** This instance runs a flow, so leaving it for good abandons that flow. */
    private var owner = false
    private lateinit var message: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        asked = savedInstanceState?.getBoolean(KEY_ASKED) == true
        val running = if (savedInstanceState == null) launch(intent) else link.state.value.canStop
        if (!running) return finish()
        owner = true
        setContentView(R.layout.activity_bootstrap)
        message = findViewById(R.id.message)
        lifecycleScope.launch { repeatOnLifecycle(Lifecycle.State.STARTED) { link.state.collect(::render) } }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        // A newer launch from the PC replaces the flow on screen; any other is ignored.
        if (launch(intent)) asked = false
    }

    private fun launch(intent: Intent?): Boolean = intent?.getStringExtra(EXTRA_SESSION)?.let(link::launch) == true

    private fun render(state: LinkState) {
        when (state) {
            LinkState.Authenticating -> message.setText(R.string.bootstrap_authenticating)
            LinkState.AwaitingConsent -> {
                message.setText(R.string.bootstrap_consent)
                if (!asked) {
                    asked = true
                    askNotificationsThenConsent()
                }
            }
            LinkState.Stopping -> message.setText(R.string.state_stopping)
            LinkState.Configuring, is LinkState.Active -> openMain()
            is LinkState.Idle -> if (state.last?.isFailure == true) openMain() else finish()
        }
    }

    private fun askNotificationsThenConsent() {
        val needed = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        // The notification only decides whether the user sees the tunnel; go on either way.
        if (needed) notifications.launch(Manifest.permission.POST_NOTIFICATIONS) else askConsent()
    }

    private fun askConsent() {
        val prompt = VpnService.prepare(this)
        if (prompt == null) link.consent(true) else consent.launch(prompt)
    }

    private fun openMain() {
        startActivity(Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        finish()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putBoolean(KEY_ASKED, asked)
    }

    override fun onDestroy() {
        if (owner && isFinishing && !isChangingConfigurations) link.abandon()
        super.onDestroy()
    }

    private companion object {
        const val EXTRA_SESSION = "session"
        const val KEY_ASKED = "asked"
    }
}
