package dev.routedroid

import android.app.Application
import android.content.Context
import android.content.Intent
import android.os.SystemClock
import androidx.core.content.ContextCompat
import com.google.android.material.color.DynamicColors
import dev.routedroid.bootstrap.HostAuthenticator
import dev.routedroid.link.DeviceLink
import dev.routedroid.transport.Clock
import dev.routedroid.vpn.RoutedroidVpnService
import java.util.concurrent.Executors

/** Owns the process-wide [DeviceLink]; everything else reaches it through [link]. */
class RoutedroidApp : Application() {
    /** Lazy: the provider can be called before [onCreate] returns. */
    val link: DeviceLink by lazy {
        DeviceLink(
            authenticator = HostAuthenticator("routedroid-android ${BuildConfig.VERSION_NAME}"),
            startVpnService = {
                ContextCompat.startForegroundService(this, Intent(this, RoutedroidVpnService::class.java).setAction(RoutedroidVpnService.ACTION_START))
            },
            clock = Clock { SystemClock.elapsedRealtime() },
            timers = Executors.newSingleThreadScheduledExecutor { r -> Thread(r, "link-timers").apply { isDaemon = true } },
        )
    }

    override fun onCreate() {
        super.onCreate()
        DynamicColors.applyToActivitiesIfAvailable(this)
    }
}

val Context.link: DeviceLink get() = (applicationContext as RoutedroidApp).link
