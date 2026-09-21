package dev.routedroid.vpn

import android.content.Intent
import android.net.VpnService
import android.util.Log
import dev.routedroid.session.PendingConnection
import dev.routedroid.session.StatusStore
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

/**
 * Foreground VpnService for one session at a time. Started by BootstrapActivity with the
 * session id after the host authenticated and the user consented; the authenticated socket
 * arrives through [PendingConnection].
 */
class RoutedroidVpnService : VpnService() {
    companion object {
        private const val TAG = "VpnService"
        const val ACTION_START = "dev.routedroid.START"
        const val ACTION_STOP = "dev.routedroid.STOP"
        const val EXTRA_SESSION = "session"
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var job: Job? = null
    @Volatile private var runner: SessionRunner? = null

    override fun onCreate() {
        super.onCreate()
        VpnNotification.createChannel(this)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> start(intent.getStringExtra(EXTRA_SESSION))
            ACTION_STOP -> runner?.stop(SessionRunner.LocalStop.USER) ?: finish()
            else -> if (job?.isActive != true) finish()
        }
        return START_NOT_STICKY
    }

    private fun start(session: String?) {
        VpnNotification.startForeground(this)
        if (job?.isActive == true) {
            Log.w(TAG, "session already running; ignoring second START")
            return
        }
        val handoff = session?.let { PendingConnection.take(it) }
        if (handoff == null) {
            // Without an authenticated connection there is nothing to run: no fallback connect.
            Log.e(TAG, "START without an authenticated host connection")
            StatusStore.setError("service started without an authenticated host connection")
            finish()
            return
        }
        val r = SessionRunner(scope, handoff, ::protect, ::Builder)
        runner = r
        job = scope.launch {
            val failure = r.run()
            if (failure != null) {
                Log.e(TAG, "session ended: $failure")
                StatusStore.setError(failure)
            } else {
                Log.i(TAG, "session ended cleanly")
            }
            runner = null
            finish()
        }
    }

    override fun onRevoke() {
        Log.w(TAG, "VPN revoked by system/user")
        StatusStore.setError("VPN permission revoked")
        runner?.stop(SessionRunner.LocalStop.REVOKED) ?: finish()
    }

    override fun onDestroy() {
        runner?.stop(SessionRunner.LocalStop.DESTROYED)
        scope.cancel()
        super.onDestroy()
    }

    private fun finish() {
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }
}
