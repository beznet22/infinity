package com.plugin.infinity_android

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.os.IBinder
import androidx.core.app.NotificationCompat

/**
 * Keeps a running turn (and its lease heartbeat) alive while the app is in the
 * background.
 *
 * A foreground service is not immortality: Android 12+ restricts starting it
 * from the background, 14+ requires a declared type, and 15 caps `dataSync` at
 * roughly six hours per day. It is the reliability layer; the lease TTL is what
 * protects other devices if this service is killed anyway.
 */
class InfinityForegroundService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val channel = NotificationChannel(
            CHANNEL_ID,
            "Infinity session",
            NotificationManager.IMPORTANCE_LOW
        ).apply {
            description = "Keeps a running agent turn alive"
            setShowBadge(false)
        }
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)

        val notification: Notification = NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("Infinity is working")
            .setContentText("A turn is running; you can leave the app.")
            .setSmallIcon(android.R.drawable.stat_sys_download)
            .setOngoing(true)
            .setSilent(true)
            .build()

        startForeground(NOTIFICATION_ID, notification)
        return START_STICKY
    }

    override fun onDestroy() {
        stopForeground(STOP_FOREGROUND_REMOVE)
        super.onDestroy()
    }

    private companion object {
        const val CHANNEL_ID = "infinity-session"
        const val NOTIFICATION_ID = 4711
    }
}
