package com.rstudio.mobile.runtime

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import com.rport.uniffi.EvalResult
import com.rport.uniffi.PlotResult
import com.rport.uniffi.RException
import com.rport.uniffi.RSession
import com.rport.uniffi.RValue
import com.rport.uniffi.RValueKind
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
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

data class EnvEntry(
    val name: String,
    val kind: String,
    val summary: String,
)

data class RStudioUiState(
    val code: String = DEFAULT_CODE,
    val console: String = R_BANNER,
    val isRunning: Boolean = false,
    val status: String = "Ready",
    val lastValue: RValue? = null,
    val lastValueSummary: String = "No result yet",
    val environment: List<EnvEntry> = emptyList(),
    val lastPlot: PlotImage? = null,
    val currentFileName: String = "untitled.R",
    val importedPath: String? = null,
)

class RStudioRuntime(private val context: Context) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val session = RSession()

    private val _state = MutableStateFlow(RStudioUiState())
    val state: StateFlow<RStudioUiState> = _state

    init {
        val bundledLibrary = File(context.filesDir, "R/bundled-library").also { it.mkdirs() }
        session.configureAndroidPaths(
            appFilesDir = context.filesDir.absolutePath,
            cacheDir = context.cacheDir.absolutePath,
            bundledLibraryDir = bundledLibrary.absolutePath,
        )
        refreshEnvironment()
    }

    fun updateCode(code: String) {
        _state.update { it.copy(code = code) }
    }

    fun runCurrentCode() {
        evaluate(_state.value.code)
    }

    fun renderCurrentCode() {
        val code = _state.value.code
        runTask("Rendering plot...") {
            val plot = withContext(Dispatchers.IO) { session.render(code, 900u, 620u) }
            setPlot(plot)
            appendConsole("Rendered plot ${plot.width}x${plot.height}")
        }
    }

    fun importCsv(uri: Uri) {
        runTask("Importing CSV...") {
            val copied = withContext(Dispatchers.IO) { copyUriToWorkspace(uri) }
            val variable = safeName(copied.nameWithoutExtension).ifBlank { "imported_csv" }
            val code = "$variable <- read.csv(\"${escapeRString(copied.absolutePath)}\")\n$variable"
            val result = withContext(Dispatchers.IO) { session.evalResult(code) }
            appendConsole("> import CSV ${copied.name}")
            publishResult(result)
            refreshEnvironmentNow()
            _state.update {
                it.copy(
                    importedPath = copied.absolutePath,
                    currentFileName = copied.name,
                    status = "Imported ${copied.name} as $variable",
                )
            }
        }
    }

    fun refreshEnvironment() {
        scope.launch {
            refreshEnvironmentNow()
        }
    }

    fun clearConsole() {
        _state.update { it.copy(console = "") }
    }

    fun cancel() {
        session.cancelCurrentOperation()
        _state.update { it.copy(isRunning = false, status = "Cancelled") }
        appendConsole("Cancelled current operation")
    }

    fun close() {
        session.close()
        scope.cancel()
    }

    private fun evaluate(code: String) {
        if (code.isBlank()) return
        runTask("Running...") {
            appendConsole("> ${code.lineSequence().firstOrNull().orEmpty()}")
            val result = withContext(Dispatchers.IO) { session.evalResult(code) }
            publishResult(result)
            refreshEnvironmentNow()
        }
    }

    private fun runTask(status: String, block: suspend () -> Unit) {
        if (_state.value.isRunning) return
        _state.update { it.copy(isRunning = true, status = status) }
        scope.launch {
            try {
                block()
                _state.update { it.copy(isRunning = false, status = "Ready") }
            } catch (error: RException) {
                markError(error.message ?: error.javaClass.simpleName)
            } catch (error: Exception) {
                markError(error.message ?: error.javaClass.simpleName)
            }
        }
    }

    private fun publishResult(result: EvalResult) {
        if (result.output.isNotBlank()) {
            appendConsole(result.output.trimEnd())
        }
        _state.update {
            it.copy(
                lastValue = result.value,
                lastValueSummary = result.value.summary(),
                status = "Ready",
            )
        }
    }

    private fun setPlot(plot: PlotResult) {
        _state.update {
            it.copy(
                lastPlot = PlotImage(plot.width.toInt(), plot.height.toInt(), plot.pixels),
                status = "Ready",
            )
        }
    }

    private suspend fun refreshEnvironmentNow() {
        val namesResult = withContext(Dispatchers.IO) { session.evalResult("ls(all.names = TRUE)") }
        val names = namesResult.value.stringValues.filterNotNull()
        val entries = names.take(80).map { name ->
            val value = withContext(Dispatchers.IO) {
                session.evalResult("get(\"${escapeRString(name)}\", envir = .GlobalEnv)")
            }.value
            EnvEntry(
                name = name,
                kind = value.kind.displayName,
                summary = value.summary(maxItems = 4),
            )
        }
        _state.update { it.copy(environment = entries) }
    }

    private fun appendConsole(line: String) {
        _state.update { state ->
            val separator = if (state.console.isBlank()) "" else "\n"
            state.copy(console = state.console + separator + line)
        }
    }

    private fun markError(message: String) {
        _state.update { it.copy(isRunning = false, status = "Error", lastValueSummary = message) }
        appendConsole("Error: $message")
    }

    private fun copyUriToWorkspace(uri: Uri): File {
        val displayName = context.contentResolver.query(uri, null, null, null, null)?.use { cursor ->
            val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
            if (index >= 0 && cursor.moveToFirst()) cursor.getString(index) else null
        } ?: "import.csv"

        val safeDisplayName = displayName.replace(Regex("[^A-Za-z0-9._-]"), "_")
        val importsDir = File(context.filesDir, "imports").also { it.mkdirs() }
        val destination = File(importsDir, safeDisplayName)

        context.contentResolver.openInputStream(uri).use { input ->
            requireNotNull(input) { "Could not open selected file" }
            destination.outputStream().use { output -> input.copyTo(output) }
        }
        return destination
    }
}

val RValueKind.displayName: String
    get() = name.lowercase().replace('_', ' ')

fun RValue.summary(maxItems: Int = 6): String {
    fun <T> List<T?>.preview(render: (T?) -> String): String =
        take(maxItems).joinToString(", ") { render(it) } + if (size > maxItems) ", ..." else ""

    val base = when (kind) {
        RValueKind.NULL -> "NULL"
        RValueKind.LOGICAL -> logicalScalar?.toString() ?: "NA"
        RValueKind.INTEGER -> integerScalar?.toString() ?: "NA"
        RValueKind.REAL -> realScalar?.toString() ?: "NA"
        RValueKind.LOGICAL_VECTOR -> "logical[${logicalValues.size}] ${logicalValues.preview { it?.toString() ?: "NA" }}"
        RValueKind.INTEGER_VECTOR -> "integer[${integerValues.size}] ${integerValues.preview { it?.toString() ?: "NA" }}"
        RValueKind.REAL_VECTOR -> "real[${realValues.size}] ${realValues.preview { it?.toString() ?: "NA" }}"
        RValueKind.STRING_VECTOR -> "character[${stringValues.size}] ${stringValues.preview { it?.let { value -> "\"$value\"" } ?: "NA" }}"
        RValueKind.RAW_VECTOR -> "raw[${rawValues.size}]"
        RValueKind.COMPLEX_VECTOR -> "complex[${complexValues.size}]"
        RValueKind.LIST -> "list[${listValues.size}]"
        RValueKind.UNSUPPORTED -> typeName.ifBlank { "unsupported" }
        RValueKind.ERROR -> error
    }

    val classes = metadata.`class`?.filterNotNull().orEmpty()
    val dims = metadata.dim.orEmpty()
    return buildString {
        if (classes.isNotEmpty()) append(classes.joinToString("/", prefix = "[", postfix = "] "))
        append(base)
        if (dims.isNotEmpty()) append(" dim=${dims.joinToString("x")}")
    }
}

private fun safeName(name: String): String {
    val cleaned = name.replace(Regex("[^A-Za-z0-9_.]"), "_")
    val prefixed = if (cleaned.firstOrNull()?.isLetter() == true || cleaned.startsWith(".")) cleaned else "data_$cleaned"
    return prefixed.ifBlank { "imported_csv" }
}

private fun escapeRString(value: String): String =
    value.replace("\\", "\\\\").replace("\"", "\\\"")

private const val DEFAULT_CODE = """# Try real R code
x <- c(1, 2, 3, 4)
sum(x)
"""

private const val R_BANNER = """RPort Android
Real Rust-backed R session ready.
"""
