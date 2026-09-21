package dev.routedroid.vpn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import dev.routedroid.R
import dev.routedroid.ui.MainActivity

/** The ongoing foreground-service notification for the tunnel. */
object VpnNotification {
    private const val CHANNEL_ID = "tunnel"
    private const val ID = 1

    fun createChannel(service: Service) {
        service.getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL_ID, service.getString(R.string.notification_channel_name), NotificationManager.IMPORTANCE_LOW)
        )
    }

    fun startForeground(service: Service) {
        val open = PendingIntent.getActivity(
            service, 0, Intent(service, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val notification: Notification = Notification.Builder(service, CHANNEL_ID)
            .setContentTitle(service.getString(R.string.notification_title))
            .setContentText(service.getString(R.string.notification_text))
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setContentIntent(open)
            .setOngoing(true)
            .build()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            service.startForeground(ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            service.startForeground(ID, notification)
        }
    }
}
