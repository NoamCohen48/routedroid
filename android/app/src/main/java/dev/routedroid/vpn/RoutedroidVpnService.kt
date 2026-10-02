package dev.routedroid.vpn

import android.content.Intent
import android.net.VpnService
import dev.routedroid.link
import dev.routedroid.link.LinkState
import dev.routedroid.link.VpnHost
import dev.routedroid.protocol.message.ConfigureVpn
import dev.routedroid.transport.PacketDevice
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

/**
 * The foreground VpnService, as a shell: [DeviceLink][dev.routedroid.link.DeviceLink] runs
 * the session and calls back through [VpnHost]. Started only by the link once the user
 * consented; never always-on (the manifest opts out), never sticky.
 */
class RoutedroidVpnService : VpnService(), VpnHost {
    private val scope = MainScope()
    @Volatile private var lastStartId = 0
    @Volatile private var foreground = false

    override fun onCreate() {
        super.onCreate()
        LinkNotification.createChannel(this)
        scope.launch {
            link.state.collect { if (foreground && it !is LinkState.Idle) LinkNotification.update(this@RoutedroidVpnService, it) }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        lastStartId = startId
        // Always first: a service started with startForegroundService() must call this.
        LinkNotification.startForeground(this, link.state.value)
        foreground = true
        when (intent?.action) {
            ACTION_START -> if (!link.attach(this)) release()
            ACTION_STOP -> if (link.state.value.canStop) link.stop() else release()
            else -> release()
        }
        return START_NOT_STICKY
    }

    override fun establish(config: ConfigureVpn): PacketDevice = TunDevice(VpnBuilderConfig.establish(Builder(), config))

    /** Any thread. stopSelf(startId) leaves the service running if a newer start arrived. */
    override fun release() {
        foreground = false
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf(lastStartId)
    }

    override fun onRevoke() {
        // Not super: its stopSelf() would race the session's own teardown and release().
        link.revoked()
    }

    override fun onDestroy() {
        link.detach(this)
        scope.cancel()
        super.onDestroy()
    }

    companion object {
        const val ACTION_START = "dev.routedroid.action.START"
        const val ACTION_STOP = "dev.routedroid.action.STOP"
    }
}
