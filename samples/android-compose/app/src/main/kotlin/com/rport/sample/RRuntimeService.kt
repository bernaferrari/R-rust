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
import com.rport.uniffi.PackageInfo
import com.rport.uniffi.PlotResult
import com.rport.uniffi.ProgressUpdate
import com.rport.uniffi.RSession
import com.rport.uniffi.SessionCallback
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

data class PlotImage(
    val width: Int,
    val height: Int,
    val pngBytes: ByteArray,
)

data class RuntimeTabUiState(
    val name: String,
    val console: String = "",
    val isRunning: Boolean = false,
    val progress: Double = 0.0,
    val lastValueKind: String = "Null",
    val installedPackages: List<String> = emptyList(),
    val loadedPackages: List<String> = emptyList(),
    val lastPlot: PlotImage? = null,
)

class RRuntimeService : Service() {
    private val binder = RRuntimeBinder()
    private val serviceScope = CoroutineScope(Dispatchers.Default + Job())

    private lateinit var sessions: List<RSession>
    private val runningJobs = mutableListOf<Job?>(null, null)
    private val notificationManager by lazy { getSystemService(NotificationManager::class.java) }

    private val _activeTabIndex = MutableStateFlow(0)
    val activeTabIndex: StateFlow<Int> = _activeTabIndex

    private val _tabs = MutableStateFlow(
        listOf(
            RuntimeTabUiState(name = "Session A"),
            RuntimeTabUiState(name = "Session B"),
        )
    )
    val tabs: StateFlow<List<RuntimeTabUiState>> = _tabs

    inner class RRuntimeBinder : Binder() {
        fun getService(): RRuntimeService = this@RRuntimeService
    }

    private inner class SessionCallbackImpl(private val tabIndex: Int) : SessionCallback {
        override fun onProgress(update: ProgressUpdate) {
            updateTab(tabIndex) { it.copy(progress = update.progress) }
        }

        override fun onOutput(line: String) {
            appendConsole(tabIndex, line)
        }

        override fun onPlotReady(plot: PlotResult) {
            updateTab(tabIndex) {
                it.copy(
                    lastPlot = PlotImage(
                        width = plot.width.toInt(),
                        height = plot.height.toInt(),
                        pngBytes = plot.pixels,
                    )
                )
            }
        }

        override fun onEvalComplete(result: EvalResult) {
            updateTab(tabIndex) { it.copy(lastValueKind = result.value.kind.toString()) }
        }

        override fun onError(error: String) {
            appendConsole(tabIndex, "Error: $error")
        }
    }

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        installDemoPackage()

        sessions = listOf(RSession(), RSession())
        sessions.forEachIndexed { index, session ->
            session.setCallback(SessionCallbackImpl(index))
            session.configureAndroidPaths(
                appFilesDir = filesDir.absolutePath,
                cacheDir = cacheDir.absolutePath,
                bundledLibraryDir = bundledLibraryDir().absolutePath,
            )
            refreshPackages(index)
        }

        startForeground(NOTIFICATION_ID, buildNotification())
        Log.d(TAG, "R runtime service started with ${sessions.size} sessions")
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent): IBinder = binder

    override fun onDestroy() {
        super.onDestroy()
        runningJobs.forEach { it?.cancel() }
        if (::sessions.isInitialized) {
            sessions.forEach { it.close() }
        }
        serviceScope.cancel()
        Log.d(TAG, "R runtime service destroyed")
    }

    fun selectTab(index: Int) {
        if (index in _tabs.value.indices) {
            _activeTabIndex.value = index
        }
    }

    fun evaluateCode(code: String) {
        runEval(_activeTabIndex.value, code)
    }

    fun renderPlot(code: String, width: Int, height: Int) {
        val tabIndex = _activeTabIndex.value
        if (isRunning(tabIndex)) return

        markRunning(tabIndex, true)
        runningJobs[tabIndex] = serviceScope.launch {
            try {
                val plot = withContext(Dispatchers.IO) {
                    sessions[tabIndex].render(code, width.toUInt(), height.toUInt())
                }
                updateTab(tabIndex) {
                    it.copy(
                        isRunning = false,
                        progress = 1.0,
                        lastPlot = PlotImage(plot.width.toInt(), plot.height.toInt(), plot.pixels),
                    )
                }
                appendConsole(tabIndex, "Rendered plot ${plot.width}x${plot.height}")
            } catch (e: Exception) {
                markError(tabIndex, "Render error: ${e.message}")
            }
        }
    }

    fun runShowcase() {
        if (_tabs.value.any { it.isRunning }) return

        runEval(
            tabIndex = 0,
            code = """
                demo_value(41)
                demo_label(demo_object("Session A"))
                session_marker <- "A"
            """.trimIndent(),
            before = { loadDemoPackage(0) },
            after = {
                renderPlotForTab(
                    tabIndex = 0,
                    code = """plot(c(1, 2, 3, 4), c(1, 4, 9, 16), type = "l", col = "blue", lwd = 2, main = "Session A growth", xlab = "x", ylab = "x^2")""",
                    width = 720,
                    height = 480,
                )
            },
        )

        runEval(
            tabIndex = 1,
            code = """
                exists("session_marker")
                session_marker <- "B"
                session_marker
            """.trimIndent(),
            before = { loadDemoPackage(1) },
            after = {
                renderPlotForTab(
                    tabIndex = 1,
                    code = """plot(c(1, 2, 3, 4), c(3, 1, 4, 2), type = "b", col = "green", cex = 1.3, main = "Session B points", xlab = "sample", ylab = "value")""",
                    width = 720,
                    height = 480,
                )
            },
        )
    }

    fun loadDemoPackage() {
        loadDemoPackage(_activeTabIndex.value)
    }

    fun listPackages() {
        refreshPackages(_activeTabIndex.value)
    }

    fun startLongRunningEval() {
        runEval(_activeTabIndex.value, "repeat { 1 + 1 }")
    }

    fun cancelExecution() {
        val tabIndex = _activeTabIndex.value
        sessions[tabIndex].cancelCurrentOperation()
        runningJobs[tabIndex]?.cancel()
        updateTab(tabIndex) { it.copy(isRunning = false, progress = 0.0) }
        appendConsole(tabIndex, "Cancelled current evaluation")
    }

    fun clearConsole() {
        val tabIndex = _activeTabIndex.value
        updateTab(tabIndex) { it.copy(console = "") }
    }

    private fun runEval(
        tabIndex: Int,
        code: String,
        before: (() -> Unit)? = null,
        after: (() -> Unit)? = null,
    ) {
        if (isRunning(tabIndex) || code.isBlank()) return

        markRunning(tabIndex, true)
        appendConsole(tabIndex, "> ${code.lines().first()}")
        runningJobs[tabIndex] = serviceScope.launch {
            try {
                before?.invoke()
                val result = withContext(Dispatchers.IO) {
                    sessions[tabIndex].evalResult(code)
                }
                if (result.output.isNotBlank()) {
                    appendConsole(tabIndex, result.output.trimEnd())
                }
                updateTab(tabIndex) {
                    it.copy(
                        isRunning = false,
                        progress = 1.0,
                        lastValueKind = result.value.kind.toString(),
                    )
                }
                after?.invoke()
            } catch (e: Exception) {
                markError(tabIndex, e.message ?: e.javaClass.simpleName)
            }
        }
    }

    private fun renderPlotForTab(tabIndex: Int, code: String, width: Int, height: Int) {
        serviceScope.launch {
            try {
                val plot = withContext(Dispatchers.IO) {
                    sessions[tabIndex].render(code, width.toUInt(), height.toUInt())
                }
                updateTab(tabIndex) {
                    it.copy(lastPlot = PlotImage(plot.width.toInt(), plot.height.toInt(), plot.pixels))
                }
                appendConsole(tabIndex, "Rendered showcase plot ${plot.width}x${plot.height}")
            } catch (e: Exception) {
                appendConsole(tabIndex, "Render error: ${e.message}")
            }
        }
    }

    private fun loadDemoPackage(tabIndex: Int) {
        sessions[tabIndex].loadPackage("androiddemo")
        updateTab(tabIndex) { tab ->
            tab.copy(loadedPackages = (tab.loadedPackages + "androiddemo").distinct())
        }
        appendConsole(tabIndex, "Loaded package androiddemo")
    }

    private fun refreshPackages(tabIndex: Int) {
        val packages = sessions[tabIndex].installedPackages()
        updateTab(tabIndex) { it.copy(installedPackages = packages.toDisplayNames()) }
    }

    private fun List<PackageInfo>.toDisplayNames(): List<String> =
        map { pkg -> "${pkg.name} ${pkg.version}" }

    private fun isRunning(tabIndex: Int): Boolean = _tabs.value[tabIndex].isRunning

    private fun markRunning(tabIndex: Int, running: Boolean) {
        updateTab(tabIndex) { it.copy(isRunning = running, progress = if (running) 0.0 else it.progress) }
    }

    private fun markError(tabIndex: Int, message: String) {
        updateTab(tabIndex) { it.copy(isRunning = false, progress = 0.0) }
        appendConsole(tabIndex, "Error: $message")
    }

    private fun appendConsole(tabIndex: Int, line: String) {
        updateTab(tabIndex) { tab ->
            val separator = if (tab.console.isBlank()) "" else "\n"
            tab.copy(console = tab.console + separator + line)
        }
    }

    private fun updateTab(tabIndex: Int, transform: (RuntimeTabUiState) -> RuntimeTabUiState) {
        _tabs.update { tabs ->
            tabs.mapIndexed { index, tab ->
                if (index == tabIndex) transform(tab) else tab
            }
        }
    }

    private fun installDemoPackage() {
        val packageDir = File(bundledLibraryDir(), "androiddemo")
        val rDir = File(packageDir, "R")
        rDir.mkdirs()

        File(packageDir, "DESCRIPTION").writeText(
            """
            Package: androiddemo
            Version: 0.1.0
            Title: RPort Android Demo Package
            Description: Pure-R package bundled with the Android showcase.
            License: MIT
            Encoding: UTF-8
            NeedsCompilation: no
            """.trimIndent() + "\n"
        )
        File(packageDir, "NAMESPACE").writeText(
            """
            export(demo_value, demo_object, demo_label)
            S3method(demo_label,androiddemo)
            """.trimIndent() + "\n"
        )
        File(rDir, "demo.R").writeText(
            """
            demo_value <- function(x = 41) x + 1
            demo_object <- function(name = "android") { x <- 1L; class(x) <- "androiddemo"; x }
            demo_label <- function(x) UseMethod("demo_label", x)
            demo_label.androiddemo <- function(x) "S3 dispatch: androiddemo"
            """.trimIndent() + "\n"
        )
    }

    private fun bundledLibraryDir(): File = File(filesDir, "R/bundled-library")

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
            .setContentText("Two isolated R sessions are available")
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
