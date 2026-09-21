package dev.routedroid

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.util.Log
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import dev.routedroid.bootstrap.BootstrapStore
import dev.routedroid.bootstrap.HostHandshake
import dev.routedroid.bootstrap.LaunchGate
import dev.routedroid.protocol.Protocol
import dev.routedroid.session.PendingConnection
import dev.routedroid.session.StatusStore
import dev.routedroid.ui.MainActivity
import dev.routedroid.vpn.RoutedroidVpnService
import kotlin.concurrent.thread

/**
 * §7.2 entry point, started by the host after it streamed the bootstrap record:
 *
 * ```
 * adb shell am start -n dev.routedroid/.BootstrapActivity --es session <id> --ei device_port <n>
 * ```
 *
 * Order (§5 step 4): take the pending record (fail closed if absent/expired/mismatched) →
 * connect and mutually authenticate the host → only then notification permission, VPN consent
 * and the foreground service. A launch without a matching record does nothing observable.
 */
class BootstrapActivity : AppCompatActivity() {
    companion object {
        private const val TAG = "Bootstrap"
        const val EXTRA_SESSION = "session"
        const val EXTRA_DEVICE_PORT = "device_port"
        private const val REQUEST_VPN_CONSENT = 1
        private const val REQUEST_NOTIFICATIONS = 2
    }

    private var session: String = ""
    private lateinit var message: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_bootstrap)
        message = findViewById(R.id.message)

        session = intent?.getStringExtra(EXTRA_SESSION) ?: ""
        val devicePort = intent?.getIntExtra(EXTRA_DEVICE_PORT, -1) ?: -1
        if (!Protocol.validSession(session) || devicePort !in 1..65535) {
            Log.e(TAG, "missing/invalid extras")
            message.text = getString(R.string.bootstrap_missing_extras)
            return // show usage, do nothing else
        }
        if (savedInstanceState != null) return // re-created mid-flow: the flow already ran
        if (!LaunchGate.tryAcquire()) {
            Log.w(TAG, "rate limited")
            message.text = getString(R.string.bootstrap_rate_limited)
            finish()
            return
        }
        val record = BootstrapStore.take(session)
        if (record == null) {
            // Fail closed: no connection, no prompt, no service, no state.
            Log.w(TAG, "refusing launch: no valid bootstrap record for session")
            message.text = getString(R.string.bootstrap_no_record)
            finish()
            return
        }
        StatusStore.reset(session, devicePort)
        message.text = getString(R.string.bootstrap_authenticating)
        thread(name = "bootstrap-auth") {
            val result = try {
                HostHandshake.run(record, devicePort)
            } catch (e: Exception) {
                Log.w(TAG, "host authentication failed: $e")
                StatusStore.setError("host authentication failed: ${e.message}")
                StatusStore.closed()
                runOnUiThread { message.text = getString(R.string.bootstrap_auth_failed); finish() }
                return@thread
            }
            PendingConnection.put(PendingConnection.Handoff(session, result.channel, result.mtu))
            runOnUiThread {
                message.text = getString(R.string.bootstrap_permissions)
                requestNotificationsThenVpn()
            }
        }
    }

    private fun requestNotificationsThenVpn() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            // Only affects whether the foreground notification is visible; proceed either way.
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), REQUEST_NOTIFICATIONS)
        } else {
            requestVpnConsent()
        }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == REQUEST_NOTIFICATIONS) requestVpnConsent()
    }

    private fun requestVpnConsent() {
        val consent = VpnService.prepare(this)
        if (consent == null) {
            startTunnel()
        } else {
            @Suppress("DEPRECATION")
            startActivityForResult(consent, REQUEST_VPN_CONSENT)
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_VPN_CONSENT) return
        if (resultCode == Activity.RESULT_OK) {
            startTunnel()
        } else {
            Log.w(TAG, "VPN consent denied")
            ConsentDenied.report(session)
            message.text = getString(R.string.bootstrap_vpn_denied)
        }
    }

    private fun startTunnel() {
        val svc = Intent(this, RoutedroidVpnService::class.java)
            .setAction(RoutedroidVpnService.ACTION_START)
            .putExtra(RoutedroidVpnService.EXTRA_SESSION, session)
        ContextCompat.startForegroundService(this, svc)
        startActivity(Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        finish()
    }
}
