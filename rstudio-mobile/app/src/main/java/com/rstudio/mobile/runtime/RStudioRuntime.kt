package com.rstudio.mobile.runtime

import android.app.Application
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import com.rstudio.mobile.data.ProjectFile
import com.rstudio.mobile.data.ProjectRepository
import com.rstudio.mobile.data.WorkspaceProject
import com.rport.uniffi.EvalResult
import com.rport.uniffi.PlotResult
import com.rport.uniffi.RException
import com.rport.uniffi.RSession
import com.rport.uniffi.RValue
import com.rport.uniffi.RValueKind
import com.rport.uniffi.PackageInfo
import com.rport.uniffi.ProgressUpdate
import com.rport.uniffi.SessionCallback
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.ViewModel
import androidx.core.content.FileProvider
import androidx.lifecycle.ViewModelProvider
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.Job
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

data class PlotImage(
    val id: Long = System.nanoTime(),
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
    val consoleHistory: List<String> = emptyList(),
    val isRunning: Boolean = false,
    val progress: Double = 0.0,
    val status: String = "Ready",
    val errorMessage: String? = null,
    val lastValue: RValue? = null,
    val lastValueSummary: String = "No result yet",
    val dataTable: DataTable? = null,
    val environment: List<EnvEntry> = emptyList(),
    val lastPlot: PlotImage? = null,
    val plots: List<PlotImage> = emptyList(),
    val packages: List<PackageInfo> = emptyList(),
    val loadedPackages: Set<String> = emptySet(),
    val currentFileName: String = "untitled.R",
    val currentScriptPath: String? = null,
    val currentDocumentUri: String? = null,
    val isDirty: Boolean = false,
    val recentScripts: List<ScriptFile> = emptyList(),
    val importedPath: String? = null,
    val projectName: String? = null,
    val projectTreeUri: String? = null,
    val projectRoot: String? = null,
    val projectFiles: List<ProjectFile> = emptyList(),
    val helpResult: String? = null,
    val helpLoading: Boolean = false,
)

class RStudioRuntime(application: Application) : AndroidViewModel(application) {
    private val context = application.applicationContext
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val projects = ProjectRepository(context.applicationContext)
    private val session = RSession()
    private val awaitingAsyncEvaluation = AtomicBoolean(false)
    private var recoveryJob: Job? = null

    private val _state = MutableStateFlow(
        projects.restoreRecovery()?.let { recovered ->
            RStudioUiState(
                code = recovered.code,
                currentFileName = recovered.name,
                currentDocumentUri = recovered.sourceUri,
                isDirty = true,
                status = "Recovered unsaved work",
            )
        } ?: RStudioUiState()
    )
    val state: StateFlow<RStudioUiState> = _state

    init {
        val bundledLibrary = File(context.filesDir, "R/bundled-library").also { it.mkdirs() }
        session.configureAndroidPaths(
            appFilesDir = context.filesDir.absolutePath,
            cacheDir = context.cacheDir.absolutePath,
            bundledLibraryDir = bundledLibrary.absolutePath,
        )
        session.setCallback(object : SessionCallback {
            override fun onProgress(update: ProgressUpdate) {
                if (awaitingAsyncEvaluation.get()) _state.update { it.copy(progress = update.progress) }
            }

            override fun onOutput(line: String) {
                if (awaitingAsyncEvaluation.get() && line.isNotBlank()) appendConsole(line.trimEnd())
            }

            override fun onPlotReady(plot: PlotResult) = Unit

            override fun onEvalComplete(result: EvalResult) {
                if (!awaitingAsyncEvaluation.compareAndSet(true, false)) return
                publishResult(result, includeOutput = false)
                _state.update { it.copy(isRunning = false, progress = 1.0, status = "Ready") }
                refreshEnvironment()
            }

            override fun onError(error: String) {
                if (awaitingAsyncEvaluation.compareAndSet(true, false)) markError(error)
            }
        })
        refreshEnvironment()
        refreshRecentScripts()
        refreshPackages()
        restoreProject()
    }

    fun updateCode(code: String) {
        _state.update { it.copy(code = code, isDirty = true) }
        recoveryJob?.cancel()
        recoveryJob = scope.launch {
            delay(500)
            val state = _state.value
            withContext(Dispatchers.IO) {
                projects.saveRecovery(state.currentFileName, state.currentDocumentUri, state.code)
            }
        }
    }

    fun runCurrentCode() {
        runCode(_state.value.code)
    }

    fun runCode(code: String) {
        if (code.isBlank() || _state.value.isRunning) return
        appendConsole("> ${code.lineSequence().firstOrNull().orEmpty()}")
        _state.update { it.copy(isRunning = true, progress = 0.0, status = "Running…", errorMessage = null) }
        awaitingAsyncEvaluation.set(true)
        scope.launch {
            try {
                withContext(Dispatchers.IO) { session.evalAsync(code) }
            } catch (error: Exception) {
                awaitingAsyncEvaluation.set(false)
                markError(error.message ?: error.javaClass.simpleName)
            }
        }
    }

    fun evaluateConsole(code: String) {
        if (code.isBlank()) return
        _state.update { state ->
            state.copy(consoleHistory = (state.consoleHistory + code).takeLast(MAX_CONSOLE_HISTORY))
        }
        runCode(code)
    }

    fun newScript() {
        _state.update {
            it.copy(
                code = "",
                currentFileName = "untitled.R",
                currentScriptPath = null,
                currentDocumentUri = null,
                isDirty = false,
                status = "New script",
                errorMessage = null,
            )
        }
        projects.clearRecovery()
    }

    fun openScript(uri: Uri) {
        runTask("Opening script...") {
            val opened = withContext(Dispatchers.IO) { projects.readText(uri) }
            val name = displayName(uri, "script.R").sanitizeFileName()
            _state.update {
                it.copy(
                    code = opened,
                    currentFileName = name,
                    currentScriptPath = null,
                    currentDocumentUri = uri.toString(),
                    isDirty = false,
                    status = "Opened $name",
                    errorMessage = null,
                )
            }
            appendConsole("Opened script $name")
        }
    }

    fun saveScriptLocal() {
        runTask("Saving script...") {
            val state = _state.value
            if (state.currentDocumentUri != null) {
                withContext(Dispatchers.IO) {
                    projects.writeText(Uri.parse(state.currentDocumentUri), state.currentScriptPath, state.code)
                    projects.clearRecovery()
                }
                _state.update { it.copy(status = "Saved ${it.currentFileName}", isDirty = false, errorMessage = null) }
                appendConsole("Saved script ${state.currentFileName}")
                return@runTask
            }
            val project = state.toWorkspaceProject()
            if (project != null) {
                val created = withContext(Dispatchers.IO) {
                    projects.createScript(project, state.currentFileName, state.code)
                }
                withContext(Dispatchers.IO) { projects.clearRecovery() }
                _state.update {
                    it.copy(
                        currentFileName = created.name,
                        currentScriptPath = created.localPath,
                        currentDocumentUri = created.uri,
                        projectFiles = (it.projectFiles + created).sortedBy(ProjectFile::relativePath),
                        isDirty = false,
                        status = "Created ${created.name}",
                        errorMessage = null,
                    )
                }
                appendConsole("Created project script ${created.name}")
                return@runTask
            }
            val saved = withContext(Dispatchers.IO) {
                saveScriptToWorkspace(state.currentFileName, state.code)
            }
            withContext(Dispatchers.IO) { projects.clearRecovery() }
            refreshRecentScriptsNow()
            _state.update {
                it.copy(
                    currentFileName = saved.name,
                    currentScriptPath = saved.absolutePath,
                    isDirty = false,
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
            withContext(Dispatchers.IO) { projects.clearRecovery() }
            _state.update {
                it.copy(
                    currentFileName = name,
                    currentScriptPath = null,
                    currentDocumentUri = uri.toString(),
                    isDirty = false,
                    status = "Exported $name",
                    errorMessage = null,
                )
            }
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
                    currentDocumentUri = null,
                    isDirty = false,
                    status = "Opened ${file.name}",
                    errorMessage = null,
                )
            }
            appendConsole("Opened script ${file.name}")
        }
    }

    fun openProject(uri: Uri) {
        runTask("Opening folder…") {
            val project = withContext(Dispatchers.IO) { projects.openProject(uri) }
            activateProject(project)
            appendConsole("Opened project ${project.name}")
        }
    }

    fun closeProject() {
        projects.clearProject()
        _state.update {
            it.copy(
                projectName = null,
                projectTreeUri = null,
                projectRoot = null,
                projectFiles = emptyList(),
                status = "Project closed",
            )
        }
    }

    fun openProjectFile(file: ProjectFile) {
        if (file.isDirectory) return
        val extension = file.name.substringAfterLast('.', "").lowercase()
        if (extension in setOf("csv", "tsv", "txt")) {
            importLocalData(File(file.localPath), separator = if (extension == "tsv") "\\t" else ",")
            return
        }
        runTask("Opening ${file.name}…") {
            val code = withContext(Dispatchers.IO) { projects.readText(file) }
            _state.update {
                it.copy(
                    code = code,
                    currentFileName = file.name,
                    currentScriptPath = file.localPath,
                    currentDocumentUri = file.uri,
                    isDirty = false,
                    status = "Opened ${file.name}",
                    errorMessage = null,
                )
            }
            appendConsole("Opened ${file.relativePath}")
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

    fun savePlotTo(uri: Uri) {
        val plot = _state.value.lastPlot ?: return
        runTask("Exporting plot…") {
            withContext(Dispatchers.IO) {
                context.contentResolver.openOutputStream(uri, "wt").use { output ->
                    requireNotNull(output) { "Could not open plot destination" }
                    output.write(plot.pngBytes)
                }
            }
            appendConsole("Exported plot ${plot.width}x${plot.height}")
        }
    }

    fun sharePlot() {
        val plot = _state.value.lastPlot ?: return
        runTask("Preparing plot…") {
            val file = withContext(Dispatchers.IO) {
                File(context.cacheDir, "shared-plots/Rplot-${plot.id}.png").apply {
                    parentFile?.mkdirs()
                    writeBytes(plot.pngBytes)
                }
            }
            val contentUri = FileProvider.getUriForFile(context, "${context.packageName}.files", file)
            val share = Intent(Intent.ACTION_SEND).apply {
                type = "image/png"
                putExtra(Intent.EXTRA_STREAM, contentUri)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
            context.startActivity(Intent.createChooser(share, "Share R plot").addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            _state.update { it.copy(status = "Plot ready to share") }
        }
    }

    fun selectPlot(id: Long) {
        _state.update { state -> state.copy(lastPlot = state.plots.firstOrNull { it.id == id } ?: state.lastPlot) }
    }

    fun importCsv(uri: Uri) {
        runTask("Importing data…") {
            val name = displayName(uri, "import.csv")
            val copied = withContext(Dispatchers.IO) {
                projects.importFile(uri, name, _state.value.toWorkspaceProject())
            }
            val variable = safeName(copied.nameWithoutExtension).ifBlank { "imported_csv" }
            val path = escapeRString(copied.absolutePath)
            val code = when (copied.extension.lowercase()) {
                "tsv" -> "$variable <- read.delim(\"$path\")\n$variable"
                "txt" -> "$variable <- read.table(\"$path\", header = TRUE)\n$variable"
                "rds" -> "$variable <- readRDS(\"$path\")\n$variable"
                "rda", "rdata" -> "load(\"$path\")\nls(all.names = TRUE)"
                else -> "$variable <- read.csv(\"$path\")\n$variable"
            }
            val result = withContext(Dispatchers.IO) { session.evalResult(code) }
            appendConsole("> import CSV ${copied.name}")
            publishResult(result)
            refreshEnvironmentNow()
            _state.update {
                it.copy(
                    importedPath = copied.absolutePath,
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

    fun inspectEnvironment(name: String) {
        runTask("Inspecting $name…") {
            val value = withContext(Dispatchers.IO) {
                session.evalResult("get(\"${escapeRString(name)}\", envir = .GlobalEnv)")
            }
            publishResult(value)
            _state.update { state ->
                state.copy(dataTable = state.dataTable?.copy(title = name), status = "Inspecting $name")
            }
        }
    }

    fun refreshPackages() {
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { session.installedPackages() } }
                .onSuccess { packages -> _state.update { it.copy(packages = packages) } }
                .onFailure { error -> markError("Could not list packages: ${error.message}") }
        }
    }

    fun loadPackage(name: String) {
        runTask("Loading $name…") {
            withContext(Dispatchers.IO) { session.loadPackage(name) }
            _state.update { it.copy(loadedPackages = it.loadedPackages + name) }
            appendConsole("Loaded package $name")
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
        awaitingAsyncEvaluation.set(false)
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

    private fun importLocalData(file: File, separator: String) {
        runTask("Importing ${file.name}…") {
            val variable = safeName(file.nameWithoutExtension).ifBlank { "imported_data" }
            val reader = if (separator == ",") "read.csv" else "read.table"
            val extra = if (separator == ",") "" else ", sep = \"$separator\", header = TRUE"
            val code = "$variable <- $reader(\"${escapeRString(file.absolutePath)}\"$extra)\n$variable"
            val result = withContext(Dispatchers.IO) { session.evalResult(code) }
            appendConsole("> import ${file.name}")
            publishResult(result)
            refreshEnvironmentNow()
            _state.update { it.copy(importedPath = file.absolutePath, status = "Imported ${file.name} as $variable") }
        }
    }

    private fun restoreProject() {
        scope.launch {
            val project = withContext(Dispatchers.IO) { projects.restoreProject() } ?: return@launch
            runCatching { activateProject(project) }
                .onFailure { error -> markError("Could not restore project: ${error.message}") }
        }
    }

    private suspend fun activateProject(project: WorkspaceProject) {
        withContext(Dispatchers.IO) {
            session.evalResult("setwd(\"${escapeRString(project.localRoot)}\")")
        }
        _state.update {
            it.copy(
                projectName = project.name,
                projectTreeUri = project.treeUri,
                projectRoot = project.localRoot,
                projectFiles = project.files,
                status = "Project: ${project.name}",
                errorMessage = null,
            )
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

    private fun publishResult(result: EvalResult, includeOutput: Boolean = true) {
        if (includeOutput && result.output.isNotBlank()) {
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
        val image = PlotImage(width = plot.width.toInt(), height = plot.height.toInt(), pngBytes = plot.pixels)
        _state.update {
            it.copy(
                lastPlot = image,
                plots = (it.plots + image).takeLast(MAX_PLOT_HISTORY),
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
            val next = state.console + separator + line
            state.copy(console = if (next.length > MAX_CONSOLE_CHARS) next.takeLast(MAX_CONSOLE_CHARS) else next)
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

class RStudioRuntimeFactory(private val application: Application) : ViewModelProvider.Factory {
    override fun <T : ViewModel> create(modelClass: Class<T>): T {
        @Suppress("UNCHECKED_CAST")
        return RStudioRuntime(application) as T
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

private fun RStudioUiState.toWorkspaceProject(): WorkspaceProject? {
    val name = projectName ?: return null
    val treeUri = projectTreeUri ?: return null
    val root = projectRoot ?: return null
    return WorkspaceProject(name, treeUri, root, projectFiles)
}

private const val DEFAULT_CODE = """# Try real R code
x <- c(1, 2, 3, 4)
sum(x)
"""

private const val R_BANNER = """RPort Android
Real Rust-backed R session ready.
"""

private const val MAX_CONSOLE_CHARS = 250_000
private const val MAX_CONSOLE_HISTORY = 100
private const val MAX_PLOT_HISTORY = 20
