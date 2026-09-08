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
import com.rport.uniffi.OperationResult
import com.rport.uniffi.OperationStatus
import com.rport.uniffi.PackageInfo
import com.rport.uniffi.PlotResult
import com.rport.uniffi.ProgressUpdate
import com.rport.uniffi.RSession
import com.rport.uniffi.SessionCallback
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
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
    val activeOperationId: ULong? = null,
    val progress: Double = 0.0,
    val lastValueKind: String = "Null",
    val installedPackages: List<String> = emptyList(),
    val loadedPackages: List<String> = emptyList(),
    val lastPlot: PlotImage? = null,
)

class RRuntimeService : Service() {
    private class OperationSlot {
        var generation: Long = 0
        var id: ULong? = null
    }

    private val binder = RRuntimeBinder()
    private val serviceScope = CoroutineScope(Dispatchers.Main.immediate + SupervisorJob())

    private lateinit var sessions: List<RSession>
    private val runningJobs = mutableListOf<Job?>(null, null)
    private val operationSlots = Array(2) { OperationSlot() }
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
        override fun onProgress(operationId: ULong, update: ProgressUpdate) {
            serviceScope.launch {
                if (acceptsCallback(tabIndex, operationId)) {
                    updateTab(tabIndex) { it.copy(progress = update.progress) }
                }
            }
        }

        // Fast operations can emit output before evalAsync returns their ID.
        // Use the complete polled result in this minimal sample, avoiding both
        // lost early notifications and duplicate callback/result output.
        override fun onOutput(operationId: ULong, line: String) = Unit

        // Terminal callbacks are notifications only. The operation polling
        // path consumes the typed result by id, so a delayed callback cannot
        // complete or overwrite a newer operation.
        override fun onPlotReady(operationId: ULong, plot: PlotResult) = Unit

        override fun onEvalComplete(operationId: ULong, result: EvalResult) = Unit

        override fun onError(operationId: ULong, error: String) = Unit
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

        startRender(tabIndex, code, width, height)
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
        currentOperation(tabIndex)?.let { operationId ->
            runCatching { sessions[tabIndex].cancelOperation(operationId) }
        } ?: sessions[tabIndex].cancelCurrentOperation()
        invalidateOperation(tabIndex)
        runningJobs[tabIndex]?.cancel()
        updateTab(tabIndex) {
            it.copy(isRunning = false, activeOperationId = null, progress = 0.0)
        }
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

        val generation = beginOperation(tabIndex)
        markRunning(tabIndex)
        appendConsole(tabIndex, "> ${code.lines().first()}")
        runningJobs[tabIndex] = serviceScope.launch {
            try {
                withContext(Dispatchers.IO) { before?.invoke() }
                val operationId = withContext(Dispatchers.IO) {
                    sessions[tabIndex].evalAsync(code)
                }
                if (!registerOperation(tabIndex, generation, operationId)) {
                    runCatching { sessions[tabIndex].cancelOperation(operationId) }
                    return@launch
                }
                when (val status = awaitOperation(sessions[tabIndex], operationId)) {
                    is OperationStatus.Succeeded -> when (val result = status.result) {
                        is OperationResult.Eval -> {
                            if (!completeOperation(tabIndex, generation, operationId)) return@launch
                            if (result.result.output.isNotBlank()) {
                                appendConsole(tabIndex, result.result.output.trimEnd())
                            }
                            updateTab(tabIndex) {
                                it.copy(lastValueKind = result.result.value.kind.toString())
                            }
                            after?.invoke()
                        }

                        is OperationResult.Render ->
                            finishOperationError(
                                tabIndex,
                                generation,
                                operationId,
                                "evaluation returned a plot",
                            )
                    }

                    is OperationStatus.Cancelled -> {
                        if (completeOperation(tabIndex, generation, operationId)) {
                            appendConsole(tabIndex, "Cancelled current evaluation")
                        }
                    }

                    is OperationStatus.Failed -> finishOperationError(
                        tabIndex,
                        generation,
                        operationId,
                        status.error,
                    )

                    else -> finishOperationError(
                        tabIndex,
                        generation,
                        operationId,
                        "operation result expired",
                    )
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (completePendingOperation(tabIndex, generation)) {
                    markError(tabIndex, e.message ?: e.javaClass.simpleName)
                }
            }
        }
    }

    private fun renderPlotForTab(tabIndex: Int, code: String, width: Int, height: Int) {
        startRender(tabIndex, code, width, height)
    }

    private fun startRender(tabIndex: Int, code: String, width: Int, height: Int) {
        if (isRunning(tabIndex)) return
        val generation = beginOperation(tabIndex)
        markRunning(tabIndex)
        runningJobs[tabIndex] = serviceScope.launch {
            try {
                val operationId = withContext(Dispatchers.IO) {
                    sessions[tabIndex].renderAsync(code, width.toUInt(), height.toUInt())
                }
                if (!registerOperation(tabIndex, generation, operationId)) {
                    runCatching { sessions[tabIndex].cancelOperation(operationId) }
                    return@launch
                }
                when (val status = awaitOperation(sessions[tabIndex], operationId)) {
                    is OperationStatus.Succeeded -> when (val result = status.result) {
                        is OperationResult.Render -> {
                            if (!completeOperation(tabIndex, generation, operationId)) return@launch
                            val plot = result.result
                            updateTab(tabIndex) {
                                it.copy(
                                    lastPlot = PlotImage(
                                        width = plot.width.toInt(),
                                        height = plot.height.toInt(),
                                        pngBytes = plot.pngBytes,
                                    )
                                )
                            }
                            appendConsole(tabIndex, "Rendered showcase plot ${plot.width}x${plot.height}")
                        }

                        is OperationResult.Eval -> finishOperationError(
                            tabIndex,
                            generation,
                            operationId,
                            "render returned an evaluation",
                        )
                    }

                    is OperationStatus.Cancelled -> {
                        if (completeOperation(tabIndex, generation, operationId)) {
                            appendConsole(tabIndex, "Cancelled current render")
                        }
                    }

                    is OperationStatus.Failed -> finishOperationError(
                        tabIndex,
                        generation,
                        operationId,
                        status.error,
                    )

                    else -> finishOperationError(
                        tabIndex,
                        generation,
                        operationId,
                        "operation result expired",
                    )
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (completePendingOperation(tabIndex, generation)) {
                    markError(tabIndex, "Render error: ${e.message ?: e.javaClass.simpleName}")
                }
            }
        }
    }

    private suspend fun awaitOperation(session: RSession, operationId: ULong): OperationStatus {
        while (true) {
            when (val status = withContext(Dispatchers.IO) { session.takeResult(operationId) }) {
                OperationStatus.Queued,
                OperationStatus.Running,
                OperationStatus.Cancelling,
                -> delay(10)

                else -> return status
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

    private fun beginOperation(tabIndex: Int): Long = synchronized(operationSlots[tabIndex]) {
        operationSlots[tabIndex].generation += 1
        operationSlots[tabIndex].id = null
        operationSlots[tabIndex].generation
    }

    private fun registerOperation(tabIndex: Int, generation: Long, operationId: ULong): Boolean {
        val accepted = synchronized(operationSlots[tabIndex]) {
            val slot = operationSlots[tabIndex]
            if (slot.generation != generation || slot.id != null) {
                false
            } else {
                slot.id = operationId
                true
            }
        }
        if (accepted) {
            updateTab(tabIndex) { it.copy(activeOperationId = operationId) }
        }
        return accepted
    }

    private fun acceptsCallback(tabIndex: Int, operationId: ULong): Boolean =
        synchronized(operationSlots[tabIndex]) {
            operationSlots[tabIndex].id == operationId
        }

    private fun currentOperation(tabIndex: Int): ULong? =
        synchronized(operationSlots[tabIndex]) { operationSlots[tabIndex].id }

    private fun invalidateOperation(tabIndex: Int) {
        synchronized(operationSlots[tabIndex]) {
            operationSlots[tabIndex].generation += 1
            operationSlots[tabIndex].id = null
        }
    }

    private fun markRunning(tabIndex: Int) {
        updateTab(tabIndex) {
            it.copy(isRunning = true, activeOperationId = null, progress = 0.0)
        }
    }

    private fun completeOperation(tabIndex: Int, generation: Long, operationId: ULong): Boolean {
        val accepted = synchronized(operationSlots[tabIndex]) {
            val slot = operationSlots[tabIndex]
            if (slot.generation != generation || slot.id != operationId) {
                false
            } else {
                slot.id = null
                true
            }
        }
        if (accepted) {
            updateTab(tabIndex) {
                it.copy(isRunning = false, activeOperationId = null, progress = 1.0)
            }
        }
        return accepted
    }

    private fun completePendingOperation(tabIndex: Int, generation: Long): Boolean {
        val accepted = synchronized(operationSlots[tabIndex]) {
            val slot = operationSlots[tabIndex]
            if (slot.generation != generation) {
                false
            } else {
                slot.id = null
                true
            }
        }
        if (accepted) {
            updateTab(tabIndex) { it.copy(activeOperationId = null) }
        }
        return accepted
    }

    private fun finishOperationError(
        tabIndex: Int,
        generation: Long,
        operationId: ULong,
        message: String,
    ) {
        if (completeOperation(tabIndex, generation, operationId)) {
            markError(tabIndex, message)
        }
    }

    private fun markError(tabIndex: Int, message: String) {
        updateTab(tabIndex) { it.copy(isRunning = false, activeOperationId = null, progress = 0.0) }
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
