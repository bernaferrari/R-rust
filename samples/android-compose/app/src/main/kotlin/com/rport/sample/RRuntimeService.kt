package com.rport.sample

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.Binder
import android.os.Build
import android.os.IBinder
import android.util.Log
import com.rport.uniffi.EvalResult
import com.rport.uniffi.ProgressUpdate
import com.rport.uniffi.RSession
import com.rport.uniffi.SessionCallback
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class RRuntimeService : Service() {
    private val binder = RRuntimeBinder()
    private val serviceScope = CoroutineScope(Dispatchers.Default + Job())

    private lateinit var rSession: RSession
    private val notificationManager by lazy { getSystemService(NotificationManager::class.java) }

    private val _sessionState = MutableStateFlow(SessionState.IDLE)
    val sessionState: StateFlow<SessionState> = _sessionState

    private val _consoleOutput = MutableStateFlow("")
    val consoleOutput: StateFlow<String> = _consoleOutput

    private val _progress = MutableStateFlow(0.0)
    val progress: StateFlow<Double> = _progress

    inner class RRuntimeBinder : Binder() {
        fun getService(): RRuntimeService = this@RRuntimeService
    }

    enum class SessionState {
        IDLE, RUNNING, CANCELLED, ERROR
    }

    private inner class SessionCallbackImpl : SessionCallback {
        override fun on_progress(update: ProgressUpdate) {
            _progress.tryEmit(update.progress)
        }

        override fun on_output(line: String) {
            _consoleOutput.tryEmit(_consoleOutput.value + line + "\n")
        }

        override fun on_plot_ready(plot: com.rport.uniffi.PlotResult) {
            // Broadcast plot ready event
        }

        override fun on_eval_complete(result: EvalResult) {
            if (result.output.isNotBlank()) {
                _consoleOutput.tryEmit(_consoleOutput.value + result.output + "\n")
            }
        }

        override fun on_error(error: String) {
            _consoleOutput.tryEmit(_consoleOutput.value + "Error: $error\n")
        }
    }

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()

        rSession = RSession()
        rSession.set_callback(SessionCallbackImpl())

        startForeground(NOTIFICATION_ID, buildNotification())
        Log.d(TAG, "R Runtime service started")
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent): IBinder {
        return binder
    }

    override fun onDestroy() {
        super.onDestroy()
        rSession.close()
        serviceScope.cancel()
        Log.d(TAG, "R Runtime service destroyed")
    }

    fun evaluateCode(code: String) {
        if (_sessionState.value == SessionState.RUNNING) return

        _sessionState.value = SessionState.RUNNING
        serviceScope.launch {
            try {
                val result = withContext(Dispatchers.IO) {
                    rSession.eval_result(code)
                }
                if (result.output.isNotBlank()) {
                    _consoleOutput.tryEmit(_consoleOutput.value + result.output + "\n")
                }
                _sessionState.value = SessionState.IDLE
            } catch (e: Exception) {
                _consoleOutput.tryEmit(_consoleOutput.value + "Error: ${e.message}\n")
                _sessionState.value = SessionState.ERROR
            }
        }
    }

    fun renderPlot(code: String, width: Int, height: Int) {
        if (_sessionState.value == SessionState.RUNNING) return

        _sessionState.value = SessionState.RUNNING
        serviceScope.launch {
            try {
                val plot = withContext(Dispatchers.IO) {
                    rSession.render(code, width.toUInt(), height.toUInt())
                }
                // Handle rendered plot
                _sessionState.value = SessionState.IDLE
            } catch (e: Exception) {
                _consoleOutput.tryEmit(_consoleOutput.value + "Render error: ${e.message}\n")
                _sessionState.value = SessionState.ERROR
            }
        }
    }

    fun cancelExecution() {
        rSession.cancel_current_operation()
        _sessionState.value = SessionState.CANCELLED
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "R Runtime Service",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "R statistical runtime execution"
                setShowBadge(false)
                enableVibration(false)
                enableLights(false)
            }
            notificationManager.createNotificationChannel(channel)
        }
    }

    private fun buildNotification(): Notification {
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, CHANNEL_ID)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }

        return builder
            .setContentTitle("R Runtime Active")
            .setContentText("Executing R code")
            .setSmallIcon(android.R.drawable.ic_menu_gallery)
            .setOngoing(true)
            .setShowWhen(false)
            .build()
    }

    companion object {
        private const val TAG = "RRuntimeService"
        private const val CHANNEL_ID = "r_runtime_service"
        private const val NOTIFICATION_ID = 1001

        fun startService(context: Context) {
            val intent = Intent(context, RRuntimeService::class.java)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        }
    }
}
