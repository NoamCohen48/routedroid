package dev.routedroid.vpn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import dev.routedroid.R
import dev.routedroid.link.LinkState
import dev.routedroid.ui.MainActivity
import dev.routedroid.ui.StatusText

/** The ongoing notification of the foreground service: what the link is doing, and Stop. */
object LinkNotification {
    private const val CHANNEL_ID = "link"
    private const val ID = 1

    fun createChannel(context: Context) {
        val channel = NotificationChannel(CHANNEL_ID, context.getString(R.string.notification_channel), NotificationManager.IMPORTANCE_LOW)
        context.getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    fun startForeground(service: Service, state: LinkState) {
        val n = build(service, state)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            service.startForeground(ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            service.startForeground(ID, n)
        }
    }

    fun update(context: Context, state: LinkState) {
        context.getSystemService(NotificationManager::class.java).notify(ID, build(context, state))
    }

    private fun build(context: Context, state: LinkState): Notification {
        val flags = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        val open = PendingIntent.getActivity(context, 0, Intent(context, MainActivity::class.java), flags)
        val stop = PendingIntent.getService(
            context, 0, Intent(context, RoutedroidVpnService::class.java).setAction(RoutedroidVpnService.ACTION_STOP), flags,
        )
        val text = StatusText(context)
        return Notification.Builder(context, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_stat_link)
            .setContentTitle(text.headline(state))
            .setContentText(text.detail(state))
            .setContentIntent(open)
            .setCategory(Notification.CATEGORY_SERVICE)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .addAction(Notification.Action.Builder(null, context.getString(R.string.stop), stop).build())
            .build()
    }
}
