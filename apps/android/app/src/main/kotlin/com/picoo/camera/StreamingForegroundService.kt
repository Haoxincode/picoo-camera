package com.picoo.camera

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat

/**
 * Keeps the sender process alive while streaming — REQ-PICOO-UI-005 / PUC-005.
 *
 * Uses PARTIAL_WAKE_LOCK for CPU; screen-on is handled by the Activity
 * (`FLAG_KEEP_SCREEN_ON`) so the FGS does not force a bright display.
 */
class StreamingForegroundService : Service() {
    private var wakeLock: PowerManager.WakeLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                stopForeground(STOP_FOREGROUND_REMOVE)
                releaseWakeLock()
                stopSelf()
                return START_NOT_STICKY
            }
            else -> {
                try {
                    acquireWakeLock()
                    val notification = buildNotification()
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                        startForeground(
                            NOTIFICATION_ID,
                            notification,
                            ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA,
                        )
                    } else {
                        startForeground(NOTIFICATION_ID, notification)
                    }
                    return START_STICKY
                } catch (error: RuntimeException) {
                    // OEM Android builds may reject a camera FGS despite the Activity
                    // being foreground. Keep the sender UI alive and surface the event
                    // in a system bug report instead of crashing the process.
                    Log.e(TAG, "Unable to start camera foreground service", error)
                    releaseWakeLock()
                    stopSelf()
                    return START_NOT_STICKY
                }
            }
        }
    }

    override fun onDestroy() {
        releaseWakeLock()
        super.onDestroy()
    }

    private fun acquireWakeLock() {
        if (wakeLock?.isHeld == true) return
        val pm = getSystemService(Context.POWER_SERVICE) as PowerManager
        wakeLock = pm.newWakeLock(
            PowerManager.PARTIAL_WAKE_LOCK,
            "PicooCamera:StreamingWakeLock",
        ).apply {
            setReferenceCounted(false)
            acquire(4 * 60 * 60 * 1000L)
        }
    }

    private fun releaseWakeLock() {
        wakeLock?.let {
            if (it.isHeld) {
                it.release()
            }
        }
        wakeLock = null
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val channel = NotificationChannel(
            CHANNEL_ID,
            "Picoo Camera streaming",
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = "Active while sending camera video to desktop"
        }
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(channel)
    }

    private fun buildNotification(): Notification {
        val launchIntent = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val stopIntent = PendingIntent.getService(
            this,
            1,
            Intent(this, StreamingForegroundService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("Picoo Camera")
            .setContentText("Streaming to desktop")
            .setSmallIcon(android.R.drawable.ic_menu_camera)
            .setContentIntent(launchIntent)
            .addAction(0, "Stop", stopIntent)
            .setOngoing(true)
            .build()
    }

    companion object {
        private const val CHANNEL_ID = "picoo_streaming"
        private const val NOTIFICATION_ID = 1001
        private const val ACTION_STOP = "com.picoo.camera.action.STOP_STREAMING"

        fun start(context: Context): Boolean {
            val intent = Intent(context, StreamingForegroundService::class.java)
            return runCatching {
                ContextCompat.startForegroundService(context, intent)
                true
            }.onFailure { Log.e(TAG, "Unable to request camera foreground service", it) }
                .getOrDefault(false)
        }

        fun stop(context: Context): Boolean {
            val intent = Intent(context, StreamingForegroundService::class.java).setAction(ACTION_STOP)
            return runCatching {
                context.startService(intent)
                true
            }.onFailure { Log.e(TAG, "Unable to stop camera foreground service", it) }
                .getOrDefault(false)
        }

        private const val TAG = "PicooStreamingService"
    }
}
