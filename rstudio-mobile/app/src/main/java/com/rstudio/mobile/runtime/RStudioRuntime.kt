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
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
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

data class DataTable(
    val title: String,
    val columns: List<String>,
    val rows: List<List<String>>,
    val totalRows: Int,
)

data class ScriptFile(
    val name: String,
    val path: String,
)

data class RStudioUiState(
    val code: String = DEFAULT_CODE,
    val console: String = R_BANNER,
    val isRunning: Boolean = false,
    val status: String = "Ready",
    val errorMessage: String? = null,
    val lastValue: RValue? = null,
    val lastValueSummary: String = "No result yet",
    val dataTable: DataTable? = null,
    val environment: List<EnvEntry> = emptyList(),
    val lastPlot: PlotImage? = null,
    val currentFileName: String = "untitled.R",
    val currentScriptPath: String? = null,
    val recentScripts: List<ScriptFile> = emptyList(),
    val importedPath: String? = null,
    val helpResult: String? = null,
    val helpLoading: Boolean = false,
)

class RStudioRuntime(private val context: Context) : ViewModel() {
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
        refreshRecentScripts()
    }

    fun updateCode(code: String) {
        _state.update { it.copy(code = code) }
    }

    fun runCurrentCode() {
        evaluate(_state.value.code)
    }

    fun newScript() {
        _state.update {
            it.copy(
                code = "",
                currentFileName = "untitled.R",
                currentScriptPath = null,
                status = "New script",
                errorMessage = null,
            )
        }
    }

    fun openScript(uri: Uri) {
        runTask("Opening script...") {
            val opened = withContext(Dispatchers.IO) { readTextUri(uri) }
            val name = displayName(uri, "script.R").sanitizeFileName()
            _state.update {
                it.copy(
                    code = opened,
                    currentFileName = name,
                    currentScriptPath = null,
                    status = "Opened $name",
                    errorMessage = null,
                )
            }
            appendConsole("Opened script $name")
        }
    }

    fun saveScriptLocal() {
        runTask("Saving script...") {
            val saved = withContext(Dispatchers.IO) {
                saveScriptToWorkspace(_state.value.currentFileName, _state.value.code)
            }
            refreshRecentScriptsNow()
            _state.update {
                it.copy(
                    currentFileName = saved.name,
                    currentScriptPath = saved.absolutePath,
                    status = "Saved ${saved.name}",
                    errorMessage = null,
                )
            }
            appendConsole("Saved script ${saved.name}")
        }
    }

    fun saveScriptTo(uri: Uri) {
        runTask("Exporting script...") {
            withContext(Dispatchers.IO) {
                context.contentResolver.openOutputStream(uri, "wt").use { output ->
                    requireNotNull(output) { "Could not open destination file" }
                    output.write(_state.value.code.toByteArray(Charsets.UTF_8))
                }
            }
            val name = displayName(uri, _state.value.currentFileName).sanitizeFileName()
            _state.update { it.copy(currentFileName = name, status = "Exported $name", errorMessage = null) }
            appendConsole("Exported script $name")
        }
    }

    fun openRecentScript(path: String) {
        runTask("Opening script...") {
            val file = File(path)
            val code = withContext(Dispatchers.IO) { file.readText() }
            _state.update {
                it.copy(
                    code = code,
                    currentFileName = file.name,
                    currentScriptPath = file.absolutePath,
                    status = "Opened ${file.name}",
                    errorMessage = null,
                )
            }
            appendConsole("Opened script ${file.name}")
        }
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

    fun evaluateHelp(topicName: String) {
        _state.update { it.copy(helpLoading = true, helpResult = null) }
        scope.launch {
            try {
                val code = """paste(capture.output(print(help("${escapeRString(topicName)}"))), collapse = "\n")"""
                val result = withContext(Dispatchers.IO) { session.evalResult(code) }
                val output = result.output.ifBlank {
                    result.value.stringValues.firstOrNull() ?: "No help available for '$topicName'"
                }
                _state.update { it.copy(helpResult = output, helpLoading = false) }
            } catch (e: Exception) {
                _state.update {
                    it.copy(helpResult = "Error loading help: ${e.message}", helpLoading = false)
                }
            }
        }
    }

    fun clearHelpResult() {
        _state.update { it.copy(helpResult = null) }
    }

    fun clearConsole() {
        _state.update { it.copy(console = "") }
    }

    fun cancel() {
        session.cancelCurrentOperation()
        _state.update { it.copy(isRunning = false, status = "Cancelled") }
        appendConsole("Cancelled current operation")
    }

    override fun onCleared() {
        super.onCleared()
        close()
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
        _state.update { it.copy(isRunning = true, status = status, errorMessage = null) }
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
                dataTable = result.value.toDataTable(),
                status = "Ready",
                errorMessage = null,
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
        _state.update {
            it.copy(
                isRunning = false,
                status = "Error",
                errorMessage = message,
                lastValueSummary = message,
            )
        }
        appendConsole("Error: $message")
    }

    private fun copyUriToWorkspace(uri: Uri): File {
        val displayName = displayName(uri, "import.csv")

        val safeDisplayName = displayName.sanitizeFileName()
        val importsDir = File(context.filesDir, "imports").also { it.mkdirs() }
        val destination = File(importsDir, safeDisplayName)

        context.contentResolver.openInputStream(uri).use { input ->
            requireNotNull(input) { "Could not open selected file" }
            destination.outputStream().use { output -> input.copyTo(output) }
        }
        return destination
    }

    private fun readTextUri(uri: Uri): String {
        context.contentResolver.openInputStream(uri).use { input ->
            requireNotNull(input) { "Could not open selected script" }
            return input.bufferedReader(Charsets.UTF_8).readText()
        }
    }

    private fun saveScriptToWorkspace(name: String, code: String): File {
        val scriptsDir = File(context.filesDir, "scripts").also { it.mkdirs() }
        val safeName = name.sanitizeFileName().let { if (it.endsWith(".R")) it else "$it.R" }
        val destination = File(scriptsDir, safeName)
        destination.writeText(code, Charsets.UTF_8)
        return destination
    }

    private fun refreshRecentScripts() {
        scope.launch { refreshRecentScriptsNow() }
    }

    private suspend fun refreshRecentScriptsNow() {
        val scripts = withContext(Dispatchers.IO) {
            File(context.filesDir, "scripts")
                .also { it.mkdirs() }
                .listFiles { file -> file.isFile && file.extension.equals("R", ignoreCase = true) }
                .orEmpty()
                .sortedByDescending { it.lastModified() }
                .take(20)
                .map { ScriptFile(it.name, it.absolutePath) }
        }
        _state.update { it.copy(recentScripts = scripts) }
    }

    private fun displayName(uri: Uri, fallback: String): String =
        context.contentResolver.query(uri, null, null, null, null)?.use { cursor ->
            val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
            if (index >= 0 && cursor.moveToFirst()) cursor.getString(index) else null
        } ?: fallback
}

class RStudioRuntimeFactory(private val context: Context) : ViewModelProvider.Factory {
    override fun <T : ViewModel> create(modelClass: Class<T>): T {
        @Suppress("UNCHECKED_CAST")
        return RStudioRuntime(context) as T
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

fun RValue.toDataTable(maxRows: Int = 200): DataTable? {
    val classes = metadata.`class`?.filterNotNull().orEmpty()
    val isDataFrame = "data.frame" in classes
    val isMatrixLike = metadata.dim.orEmpty().size == 2 && kind != RValueKind.LIST

    if (isDataFrame && kind == RValueKind.LIST && listValues.isNotEmpty()) {
        val columns = metadata.names.orEmpty()
            .mapIndexed { index, name -> name?.takeIf { it.isNotBlank() } ?: "V${index + 1}" }
        val rowCount = listValues.maxOf { it.vectorLength() }
        val rows = (0 until minOf(rowCount, maxRows)).map { row ->
            listValues.map { column -> column.valueAt(row) }
        }
        return DataTable(
            title = "data.frame ${rowCount}x${listValues.size}",
            columns = columns,
            rows = rows,
            totalRows = rowCount,
        )
    }

    if (isMatrixLike) {
        val dims = metadata.dim.orEmpty()
        val rowCount = dims[0]
        val colCount = dims[1]
        val columns = metadata.names.orEmpty().takeIf { it.size == colCount }
            ?.mapIndexed { index, name -> name?.takeIf { it.isNotBlank() } ?: "V${index + 1}" }
            ?: (1..colCount).map { "V$it" }
        val rows = (0 until minOf(rowCount, maxRows)).map { row ->
            (0 until colCount).map { col -> valueAt(row + col * rowCount) }
        }
        return DataTable("matrix ${rowCount}x$colCount", columns, rows, rowCount)
    }

    return null
}

private fun RValue.vectorLength(): Int = when (kind) {
    RValueKind.LOGICAL -> 1
    RValueKind.INTEGER -> 1
    RValueKind.REAL -> 1
    RValueKind.LOGICAL_VECTOR -> logicalValues.size
    RValueKind.INTEGER_VECTOR -> integerValues.size
    RValueKind.REAL_VECTOR -> realValues.size
    RValueKind.STRING_VECTOR -> stringValues.size
    RValueKind.RAW_VECTOR -> rawValues.size
    RValueKind.COMPLEX_VECTOR -> complexValues.size
    RValueKind.LIST -> listValues.size
    else -> 0
}

private fun RValue.valueAt(index: Int): String = when (kind) {
    RValueKind.LOGICAL -> if (index == 0) logicalScalar?.toString() ?: "NA" else ""
    RValueKind.INTEGER -> if (index == 0) integerScalar?.toString() ?: "NA" else ""
    RValueKind.REAL -> if (index == 0) realScalar?.toString() ?: "NA" else ""
    RValueKind.LOGICAL_VECTOR -> logicalValues.getOrNull(index)?.toString() ?: "NA"
    RValueKind.INTEGER_VECTOR -> integerValues.getOrNull(index)?.toString() ?: "NA"
    RValueKind.REAL_VECTOR -> realValues.getOrNull(index)?.toString() ?: "NA"
    RValueKind.STRING_VECTOR -> stringValues.getOrNull(index) ?: "NA"
    RValueKind.RAW_VECTOR -> rawValues.getOrNull(index)?.toUByte()?.toString(16)?.padStart(2, '0') ?: ""
    RValueKind.COMPLEX_VECTOR -> complexValues.getOrNull(index)?.let { "${it.real}+${it.imaginary}i" } ?: "NA"
    RValueKind.LIST -> listValues.getOrNull(index)?.summary(maxItems = 2).orEmpty()
    else -> ""
}

private fun safeName(name: String): String {
    val cleaned = name.replace(Regex("[^A-Za-z0-9_.]"), "_")
    val prefixed = if (cleaned.firstOrNull()?.isLetter() == true || cleaned.startsWith(".")) cleaned else "data_$cleaned"
    return prefixed.ifBlank { "imported_csv" }
}

private fun escapeRString(value: String): String =
    value.replace("\\", "\\\\").replace("\"", "\\\"")

private fun String.sanitizeFileName(): String =
    replace(Regex("[^A-Za-z0-9._-]"), "_").ifBlank { "untitled.R" }

private const val DEFAULT_CODE = """# Try real R code
x <- c(1, 2, 3, 4)
sum(x)
"""

private const val R_BANNER = """RPort Android
Real Rust-backed R session ready.
"""
