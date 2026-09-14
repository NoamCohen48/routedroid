package dev.routedroid.phase0

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

/**
 * Entry point used by the host:
 *
 * ```
 * adb -s SERIAL shell am start -n dev.routedroid.phase0/.BootstrapActivity \
 *     --es session <id> --ei device_port <DEVICE_PORT>
 * ```
 *
 * Exported so `am start` (shell) can launch it; deliberately carries no intent-filter so it is
 * not browsable and cannot be targeted implicitly. Phase 0 has no bootstrap secret and no host
 * authentication (implementation-plan §3.4 / architecture §8.1 add them); anyone who can start
 * this activity can trigger the VPN consent prompt. That is accepted for the throwaway probe.
 */
class BootstrapActivity : AppCompatActivity() {

    companion object {
        private const val TAG = "Phase0Bootstrap"
        private const val REQUEST_VPN_CONSENT = 1
        private const val REQUEST_NOTIFICATIONS = 2
    }

    private var session: String? = null
    private var devicePort: Int = -1
    private lateinit var message: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_bootstrap)
        message = findViewById(R.id.message)

        session = intent?.getStringExtra(Phase0VpnService.EXTRA_SESSION)
        devicePort = intent?.getIntExtra(Phase0VpnService.EXTRA_DEVICE_PORT, -1) ?: -1
        if (session.isNullOrEmpty() || devicePort !in 1..65535) {
            Log.e(TAG, "missing/invalid extras: session=$session device_port=$devicePort")
            message.text = getString(R.string.bootstrap_missing_extras)
            StatusStore.setError("bootstrap started without valid extras")
            return // show error, do nothing else
        }
        message.text = "session=$session device_port=$devicePort\nrequesting permissions..."

        if (savedInstanceState == null) requestNotificationsThenVpn()
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
            StatusStore.setError("VPN consent denied")
            message.text = getString(R.string.bootstrap_vpn_denied)
        }
    }

    private fun startTunnel() {
        val svc = Intent(this, Phase0VpnService::class.java)
            .setAction(Phase0VpnService.ACTION_START)
            .putExtra(Phase0VpnService.EXTRA_SESSION, session)
            .putExtra(Phase0VpnService.EXTRA_DEVICE_PORT, devicePort)
        ContextCompat.startForegroundService(this, svc)
        startActivity(Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        finish()
    }
}
