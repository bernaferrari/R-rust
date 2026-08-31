package com.rstudio.mobile.runtime

import android.app.Application
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import com.rstudio.mobile.data.ProjectFile
import com.rstudio.mobile.data.ProjectRepository
import com.rstudio.mobile.data.WorkspaceProject
import com.rstudio.shared.ReportChunkResult
import com.rstudio.shared.ReportRenderer
import com.rport.uniffi.DataFramePage
import com.rport.uniffi.EvalResult
import com.rport.uniffi.PlotResult
import com.rport.uniffi.RException
import com.rport.uniffi.RSession
import com.rport.uniffi.RValue
import com.rport.uniffi.RValueKind
import com.rport.uniffi.PackageInfo
import com.rport.uniffi.ProgressUpdate
import com.rport.uniffi.SessionCallback
import com.rport.uniffi.RuntimeInfo
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.ViewModel
import androidx.core.content.FileProvider
import androidx.lifecycle.ViewModelProvider
import java.io.File
import java.util.zip.ZipInputStream
import java.util.zip.ZipOutputStream
import java.util.concurrent.atomic.AtomicBoolean
import org.json.JSONArray
import org.json.JSONObject
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
    val rowOffset: Int = 0,
)

data class ScriptFile(
    val name: String,
    val path: String,
)

data class EditorDocument(
    val id: String,
    val name: String,
    val code: String,
    val sourceUri: String? = null,
    val localPath: String? = null,
    val isDirty: Boolean = false,
)

data class Diagnostic(
    val message: String,
    val line: Int? = null,
    val severity: String = "error",
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
    val documents: List<EditorDocument> = listOf(EditorDocument("untitled.R", "untitled.R", DEFAULT_CODE)),
    val activeDocumentId: String = "untitled.R",
    val diagnostics: List<Diagnostic> = emptyList(),
    val runtimeInfo: RuntimeInfo? = null,
    val dataSourceName: String? = null,
    val dataRowOffset: Int = 0,
)

class RStudioRuntime(application: Application) : AndroidViewModel(application) {
    private data class PersistedEditorState(
        val documents: List<EditorDocument>,
        val activeId: String,
        val consoleHistory: List<String>,
    )

    private val context = application.applicationContext
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val projects = ProjectRepository(context.applicationContext)
    private var session = RSession()
    private val sessionPreferences = context.getSharedPreferences("r_workbench_session", 0)
    private val awaitingAsyncEvaluation = AtomicBoolean(false)
    private var recoveryJob: Job? = null

    private val _state = MutableStateFlow(initialUiState())
    val state: StateFlow<RStudioUiState> = _state

    private fun initialUiState(): RStudioUiState {
        val persisted = restoreEditorState()
        val recovery = projects.restoreRecovery()?.let {
            RecoveryDraft(name = it.name, sourceUri = it.sourceUri, code = it.code)
        }
        return restoredEditorState(
            documents = persisted.documents,
            requestedActiveId = persisted.activeId,
            consoleHistory = persisted.consoleHistory,
            recovery = recovery,
        )
    }

    private fun restoreEditorState(): PersistedEditorState {
        val raw = sessionPreferences.getString("documents", null) ?: return PersistedEditorState(
            listOf(EditorDocument("untitled.R", "untitled.R", DEFAULT_CODE)), "untitled.R", emptyList()
        )
        return runCatching {
            val array = JSONArray(raw)
            val documents = (0 until array.length()).map { index ->
                val item = array.getJSONObject(index)
                EditorDocument(
                    id = item.getString("id"),
                    name = item.getString("name"),
                    code = item.optString("code"),
                    sourceUri = item.optString("sourceUri").takeIf { it.isNotBlank() },
                    localPath = item.optString("localPath").takeIf { it.isNotBlank() },
                    isDirty = item.optBoolean("dirty"),
                )
            }.ifEmpty { listOf(EditorDocument("untitled.R", "untitled.R", DEFAULT_CODE)) }
            val historyJson = sessionPreferences.getString("consoleHistoryJson", null)
            val history = if (historyJson != null) {
                val historyArray = JSONArray(historyJson)
                (0 until historyArray.length()).map { historyArray.getString(it) }
            } else {
                sessionPreferences.getStringSet("consoleHistory", emptySet()).orEmpty().toList()
            }
            PersistedEditorState(documents, sessionPreferences.getString("activeId", documents.first().id) ?: documents.first().id, history)
        }.getOrElse {
            PersistedEditorState(listOf(EditorDocument("untitled.R", "untitled.R", DEFAULT_CODE)), "untitled.R", emptyList())
        }
    }

    private fun persistEditorState(state: RStudioUiState) {
        val array = JSONArray()
        state.documents.forEach { document ->
            array.put(JSONObject().apply {
                put("id", document.id)
                put("name", document.name)
                put("code", document.code)
                put("sourceUri", document.sourceUri ?: "")
                put("localPath", document.localPath ?: "")
                put("dirty", document.isDirty)
            })
        }
        val history = JSONArray()
        state.consoleHistory.forEach(history::put)
        sessionPreferences.edit()
            .putString("documents", array.toString())
            .putString("activeId", state.activeDocumentId)
            .putString("consoleHistoryJson", history.toString())
            .apply()
    }

    init {
        configureSession(session)
        refreshEnvironment()
        refreshRecentScripts()
        refreshPackages()
        restoreProject()
    }

    private fun configureSession(candidate: RSession) {
        val bundledLibrary = File(context.filesDir, "R/bundled-library").also { it.mkdirs() }
        candidate.configureAndroidPaths(
            appFilesDir = context.filesDir.absolutePath,
            cacheDir = context.cacheDir.absolutePath,
            bundledLibraryDir = bundledLibrary.absolutePath,
        )
        _state.update { it.copy(runtimeInfo = runCatching { candidate.runtimeInfo() }.getOrNull()) }
        candidate.setCallback(object : SessionCallback {
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
    }

    fun updateCode(code: String) {
        _state.update { it.editActiveDocument(code) }
        recoveryJob?.cancel()
        recoveryJob = scope.launch {
            delay(500)
            val state = _state.value
            withContext(Dispatchers.IO) {
                projects.saveRecovery(state.currentFileName, state.currentDocumentUri, state.code)
                persistEditorState(state)
            }
        }
    }

    fun runCurrentCode() {
        runCode(_state.value.code)
    }

    fun runCode(code: String) {
        if (code.isBlank() || _state.value.isRunning) return
        appendConsole("> ${code.lineSequence().firstOrNull().orEmpty()}")
        _state.update { it.copy(isRunning = true, progress = 0.0, status = "Running…", errorMessage = null, diagnostics = emptyList()) }
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
        val id = "untitled-${System.nanoTime()}"
        _state.update {
            it.copy(
                code = "",
                currentFileName = "untitled.R",
                currentScriptPath = null,
                currentDocumentUri = null,
                isDirty = false,
                status = "New script",
                errorMessage = null,
                documents = it.documents + EditorDocument(id, "untitled.R", ""),
                activeDocumentId = id,
            )
        }
        persistEditorState(_state.value)
        projects.clearRecovery()
    }

    fun activateDocument(id: String) {
        val document = _state.value.documents.firstOrNull { it.id == id } ?: return
        _state.update {
            it.copy(
                code = document.code,
                currentFileName = document.name,
                currentScriptPath = document.localPath,
                currentDocumentUri = document.sourceUri,
                isDirty = document.isDirty,
                activeDocumentId = document.id,
                errorMessage = null,
            )
        }
        persistEditorState(_state.value)
    }

    fun closeDocument(id: String) {
        val state = _state.value
        if (state.documents.size <= 1) return
        val remaining = state.documents.filterNot { it.id == id }
        val next = remaining.firstOrNull { it.id == state.activeDocumentId } ?: remaining.last()
        _state.update { it.copy(documents = remaining, activeDocumentId = next.id) }
        if (state.activeDocumentId == id) activateDocument(next.id)
        persistEditorState(_state.value)
    }

    fun openScript(uri: Uri) {
        runTask("Opening script...") {
            val opened = withContext(Dispatchers.IO) { projects.readText(uri) }
            val name = displayName(uri, "script.R").sanitizeFileName()
            val id = uri.toString()
            _state.update {
                it.copy(
                    code = opened,
                    currentFileName = name,
                    currentScriptPath = null,
                    currentDocumentUri = uri.toString(),
                    isDirty = false,
                    status = "Opened $name",
                    errorMessage = null,
                    documents = it.documents.upsert(EditorDocument(id, name, opened, uri.toString())),
                    activeDocumentId = id,
                )
            }
            persistEditorState(_state.value)
            appendConsole("Opened script $name")
        }
    }

    fun saveScriptLocal() {
        runTask("Saving script...") {
            val state = _state.value
            val snapshot = state.activeSaveSnapshot()
            if (snapshot.sourceUri != null) {
                withContext(Dispatchers.IO) {
                    projects.writeText(Uri.parse(snapshot.sourceUri), snapshot.localPath, snapshot.code)
                }
                val clearRecovery = _state.value.canClearRecovery(snapshot)
                _state.update { it.completeDocumentSave(snapshot) }
                if (clearRecovery) withContext(Dispatchers.IO) { projects.clearRecovery() }
                appendConsole("Saved script ${snapshot.name}")
                return@runTask
            }
            val project = state.toWorkspaceProject()
            if (project != null) {
                val created = withContext(Dispatchers.IO) {
                    projects.createScript(project, snapshot.name, snapshot.code)
                }
                val clearRecovery = _state.value.canClearRecovery(snapshot)
                _state.update {
                    it.completeDocumentSave(
                        snapshot = snapshot,
                        savedName = created.file.name,
                        savedSourceUri = created.file.uri,
                        savedLocalPath = created.file.localPath,
                        status = "Created ${created.file.name}",
                    ).copy(
                        projectFiles = created.project.files,
                    )
                }
                if (clearRecovery) withContext(Dispatchers.IO) { projects.clearRecovery() }
                appendConsole("Created project script ${created.file.name}")
                return@runTask
            }
            val saved = withContext(Dispatchers.IO) {
                saveScriptToWorkspace(snapshot.name, snapshot.code)
            }
            val clearRecovery = _state.value.canClearRecovery(snapshot)
            refreshRecentScriptsNow()
            _state.update {
                it.completeDocumentSave(
                    snapshot = snapshot,
                    savedName = saved.name,
                    savedSourceUri = null,
                    savedLocalPath = saved.absolutePath,
                )
            }
            if (clearRecovery) withContext(Dispatchers.IO) { projects.clearRecovery() }
            appendConsole("Saved script ${saved.name}")
        }
    }

    fun saveScriptTo(uri: Uri) {
        val snapshot = _state.value.activeSaveSnapshot()
        runTask("Exporting script...") {
            withContext(Dispatchers.IO) {
                context.contentResolver.openOutputStream(uri, "wt").use { output ->
                    requireNotNull(output) { "Could not open destination file" }
                    output.write(snapshot.code.toByteArray(Charsets.UTF_8))
                }
            }
            val name = displayName(uri, snapshot.name).sanitizeFileName()
            val clearRecovery = _state.value.canClearRecovery(snapshot)
            _state.update {
                it.completeDocumentSave(
                    snapshot = snapshot,
                    savedName = name,
                    savedSourceUri = uri.toString(),
                    savedLocalPath = null,
                    status = "Exported $name",
                )
            }
            if (clearRecovery) withContext(Dispatchers.IO) { projects.clearRecovery() }
            appendConsole("Exported script $name")
        }
    }

    fun exportReportTo(uri: Uri) {
        val source = _state.value.code
        runTask("Rendering report…") {
            val html = withContext(Dispatchers.IO) {
                ReportRenderer.render(source) { chunk ->
                    runCatching { session.evalResult(chunk).output }
                        .fold(
                            onSuccess = { ReportChunkResult(chunk, it) },
                            onFailure = { ReportChunkResult(chunk, "", it.message ?: "Evaluation failed") },
                        )
                }
            }
            withContext(Dispatchers.IO) {
                context.contentResolver.openOutputStream(uri, "wt").use { output ->
                    requireNotNull(output) { "Could not open report destination" }
                    output.write(html.toByteArray(Charsets.UTF_8))
                }
            }
            appendConsole("Exported HTML report")
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
                    documents = it.documents.upsert(EditorDocument(file.absolutePath, file.name, code, null, file.absolutePath)),
                    activeDocumentId = file.absolutePath,
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

    fun exportProjectTo(uri: Uri) {
        val project = _state.value.toWorkspaceProject() ?: return
        runTask("Exporting project…") {
            withContext(Dispatchers.IO) {
                context.contentResolver.openOutputStream(uri, "wt").use { output ->
                    requireNotNull(output) { "Could not open project destination" }
                    ZipOutputStream(output.buffered()).use { zip ->
                        project.files.filterNot { it.isDirectory }.forEach { file ->
                            val local = File(file.localPath)
                            if (!local.isFile) return@forEach
                            zip.putNextEntry(java.util.zip.ZipEntry(file.relativePath))
                            local.inputStream().use { input -> input.copyTo(zip) }
                            zip.closeEntry()
                        }
                    }
                }
            }
            appendConsole("Exported project ${project.name}")
        }
    }

    fun refreshProject() {
        scope.launch {
            runCatching {
                val project = withContext(Dispatchers.IO) { projects.restoreProject() } ?: return@runCatching
                activateProject(project)
            }
                .onFailure { markError("Could not refresh project: ${it.message}") }
        }
    }

    fun createProjectFolder(name: String) {
        val project = _state.value.toWorkspaceProject() ?: return
        runTask("Creating folder…") {
            val refreshed = withContext(Dispatchers.IO) { projects.createFolder(project, name) }
            activateProject(refreshed)
        }
    }

    fun renameProjectFile(file: ProjectFile, name: String) {
        val project = _state.value.toWorkspaceProject() ?: return
        runTask("Renaming ${file.name}…") {
            val refreshed = withContext(Dispatchers.IO) {
                projects.rename(project, Uri.parse(file.uri), name)
            }
            activateProject(refreshed)
        }
    }

    fun deleteProjectFile(file: ProjectFile) {
        val project = _state.value.toWorkspaceProject() ?: return
        runTask("Deleting ${file.name}…") {
            val refreshed = withContext(Dispatchers.IO) {
                projects.delete(project, Uri.parse(file.uri))
            }
            if (_state.value.currentDocumentUri == file.uri) newScript()
            activateProject(refreshed)
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
                it.openEditorDocument(
                    EditorDocument(
                        id = file.uri,
                        name = file.name,
                        code = code,
                        sourceUri = file.uri,
                        localPath = file.localPath,
                    ),
                    status = "Opened ${file.name}",
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
            val imported = withContext(Dispatchers.IO) {
                projects.importFile(uri, name, _state.value.toWorkspaceProject())
            }
            val copied = imported.file
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
            imported.project?.let { activateProject(it) }
            _state.update {
                it.copy(
                    importedPath = copied.absolutePath,
                    dataSourceName = variable,
                    dataRowOffset = 0,
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
            val page = withContext(Dispatchers.IO) {
                try {
                    session.dataFramePage(name, 0UL, DATA_PAGE_SIZE.toULong())
                } catch (_: RException.InvalidInput) {
                    null
                }
            }
            if (page == null) {
                val value = withContext(Dispatchers.IO) {
                    session.evalResult("get(\"${escapeRString(name)}\", envir = .GlobalEnv)")
                }
                publishResult(value)
            } else {
                val table = page.toDataTable(name) ?: error("$name is not a rectangular table")
                _state.update { state ->
                    state.copy(
                        dataTable = table,
                        dataSourceName = name,
                        dataRowOffset = table.rowOffset,
                        status = "Inspecting $name",
                    )
                }
            }
        }
    }

    fun loadMoreData() {
        val name = _state.value.dataSourceName ?: return
        val offset = _state.value.dataTable?.rows?.size?.plus(_state.value.dataTable?.rowOffset ?: 0) ?: return
        runTask("Loading more rows…") {
            val page = withContext(Dispatchers.IO) {
                session.dataFramePage(name, offset.toULong(), DATA_PAGE_SIZE.toULong())
            }
            val next = page.toDataTable(name) ?: return@runTask
            _state.update { state ->
                val current = state.dataTable
                state.copy(
                    dataTable = if (current == null) next else current.copy(rows = current.rows + next.rows),
                    dataRowOffset = next.rowOffset,
                    status = "Loaded ${current?.rows?.size?.plus(next.rows.size) ?: next.rows.size} rows",
                )
            }
        }
    }

    fun removeEnvironment(name: String) {
        runTask("Removing $name…") {
            withContext(Dispatchers.IO) {
                session.evalResult("rm(list = \"${escapeRString(name)}\", envir = .GlobalEnv)")
            }
            appendConsole("Removed $name")
            refreshEnvironmentNow()
        }
    }

    fun exportDataTo(uri: Uri) {
        val table = _state.value.dataTable ?: return
        val sourceName = _state.value.dataSourceName
        runTask("Exporting data…") {
            withContext(Dispatchers.IO) {
                context.contentResolver.openOutputStream(uri, "wt").use { output ->
                    requireNotNull(output) { "Could not open data destination" }
                    output.bufferedWriter(Charsets.UTF_8).use { writer ->
                        if (sourceName == null) {
                            writeCsvPage(writer, table.columns, table.rows)
                        } else {
                            var offset = 0UL
                            var totalRows = ULong.MAX_VALUE
                            var rowsWritten: ULong
                            var wroteHeader = false
                            do {
                                val page = session.dataFramePage(sourceName, offset, EXPORT_PAGE_SIZE)
                                val next = page.toDataTable(sourceName)
                                    ?: error("$sourceName is no longer a rectangular table")
                                if (!wroteHeader) {
                                    writeCsvHeader(writer, next.columns)
                                    wroteHeader = true
                                }
                                writeCsvRows(writer, next.rows)
                                if (totalRows == ULong.MAX_VALUE) totalRows = page.totalRows
                                rowsWritten = next.rows.size.toULong()
                                offset += rowsWritten
                            } while (offset < totalRows && rowsWritten > 0UL)
                        }
                    }
                }
            }
            appendConsole("Exported ${table.title} as CSV")
        }
    }

    fun saveWorkspaceTo(uri: Uri) {
        runTask("Saving workspace…") {
            val local = File(context.cacheDir, "workspace-save.RData")
            withContext(Dispatchers.IO) {
                session.evalResult("save.image(file = \"${escapeRString(local.absolutePath)}\")")
                context.contentResolver.openOutputStream(uri, "wt").use { output ->
                    requireNotNull(output) { "Could not open workspace destination" }
                    local.inputStream().use { input -> input.copyTo(output) }
                }
                local.delete()
            }
            appendConsole("Saved workspace")
        }
    }

    fun loadWorkspace(uri: Uri) {
        runTask("Loading workspace…") {
            val local = withContext(Dispatchers.IO) { projects.importFile(uri, "workspace.RData", null).file }
            withContext(Dispatchers.IO) {
                session.evalResult("load(\"${escapeRString(local.absolutePath)}\", envir = .GlobalEnv)")
            }
            appendConsole("Loaded workspace")
            refreshEnvironmentNow()
        }
    }

    fun refreshPackages() {
        scope.launch {
            refreshPackagesNow()
        }
    }

    private suspend fun refreshPackagesNow() {
        runCatching { withContext(Dispatchers.IO) { session.installedPackages() } }
            .onSuccess { packages -> _state.update { it.copy(packages = packages) } }
            .onFailure { error -> markError("Could not list packages: ${error.message}") }
    }

    fun loadPackage(name: String) {
        runTask("Loading $name…") {
            withContext(Dispatchers.IO) { session.loadPackage(name) }
            _state.update { it.copy(loadedPackages = it.loadedPackages + name) }
            appendConsole("Loaded package $name")
        }
    }

    fun installPackage(uri: Uri) {
        runTask("Installing package…") {
            val packageName = withContext(Dispatchers.IO) {
                val staging = File(context.cacheDir, "package-install/${System.nanoTime()}").also { it.mkdirs() }
                try {
                    extractPackageZip(uri, staging)
                    val packageRoot = staging.walkTopDown()
                        .firstOrNull { it.isFile && it.name == "DESCRIPTION" }
                        ?.parentFile
                        ?: error("The archive does not contain an R package DESCRIPTION file")
                    val description = File(packageRoot, "DESCRIPTION").readText(Charsets.UTF_8)
                    val name = Regex("(?m)^Package:\\s*([^\\r\\n]+)").find(description)?.groupValues?.get(1)?.trim()
                        ?.takeIf { it.matches(Regex("[A-Za-z][A-Za-z0-9.]*")) }
                        ?: error("DESCRIPTION has no valid Package field")
                    val targetRoot = File(context.filesDir, "R/library/$name")
                    targetRoot.parentFile?.mkdirs()
                    if (targetRoot.exists()) targetRoot.deleteRecursively()
                    packageRoot.copyRecursively(targetRoot, overwrite = true)
                    name
                } finally {
                    staging.deleteRecursively()
                }
            }
            appendConsole("Installed R package $packageName")
            refreshPackagesNow()
        }
    }

    fun removePackage(packageInfo: PackageInfo) {
        runTask("Removing ${packageInfo.name}…") {
            val userLibrary = File(context.filesDir, "R/library").canonicalFile
            val packagePath = File(packageInfo.path).canonicalFile
            require(packagePath.parentFile == userLibrary) { "Only packages installed in the app library can be removed" }
            withContext(Dispatchers.IO) { packagePath.deleteRecursively() }
            _state.update { it.copy(loadedPackages = it.loadedPackages - packageInfo.name) }
            appendConsole("Removed R package ${packageInfo.name}")
            refreshPackagesNow()
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

    fun resetSession() {
        if (_state.value.isRunning) cancel()
        awaitingAsyncEvaluation.set(false)
        runCatching { session.close() }
        session = RSession()
        configureSession(session)
        _state.update {
            it.copy(
                console = R_BANNER,
                environment = emptyList(),
                dataTable = null,
                dataSourceName = null,
                dataRowOffset = 0,
                lastValue = null,
                lastValueSummary = "No result yet",
                loadedPackages = emptySet(),
                lastPlot = null,
                plots = emptyList(),
                diagnostics = emptyList(),
                errorMessage = null,
                isRunning = false,
                progress = 0.0,
                status = "Session reset",
            )
        }
        refreshEnvironment()
        refreshPackages()
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
            _state.update {
                it.copy(importedPath = file.absolutePath, dataSourceName = variable, dataRowOffset = 0, status = "Imported ${file.name} as $variable")
            }
        }
    }

    private fun restoreProject() {
        scope.launch {
            runCatching {
                val project = withContext(Dispatchers.IO) { projects.restoreProject() } ?: return@runCatching
                activateProject(project)
            }
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
            ).reconcileProjectDocuments(project.files)
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
                dataSourceName = null,
                dataRowOffset = 0,
                status = "Ready",
                errorMessage = null,
            )
        }
    }

    private fun setPlot(plot: PlotResult) {
        val image = PlotImage(width = plot.width.toInt(), height = plot.height.toInt(), pngBytes = plot.pngBytes)
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
        val line = Regex("(?i)(?:line|at)\\s+(\\d+)").find(message)?.groupValues?.getOrNull(1)?.toIntOrNull()
        _state.update {
            it.copy(
                isRunning = false,
                status = "Error",
                errorMessage = message,
                lastValueSummary = message,
                diagnostics = listOf(Diagnostic(message = message, line = line)),
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

    private fun extractPackageZip(uri: Uri, destination: File) {
        context.contentResolver.openInputStream(uri).use { input ->
            requireNotNull(input) { "Could not open package archive" }
            ZipInputStream(input.buffered()).use { zip ->
                var total = 0L
                while (true) {
                    val entry = zip.nextEntry ?: break
                    val name = entry.name.replace('\\', '/')
                    require(name.isNotBlank() && !name.startsWith("/")) { "Invalid package archive entry" }
                    val output = File(destination, name).canonicalFile
                    require(output.path.startsWith(destination.canonicalPath + File.separator)) { "Unsafe package archive path" }
                    if (entry.isDirectory) {
                        output.mkdirs()
                    } else {
                        output.parentFile?.mkdirs()
                        output.outputStream().use { out ->
                            val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
                            while (true) {
                                val count = zip.read(buffer)
                                if (count <= 0) break
                                total += count
                                require(total <= MAX_PACKAGE_BYTES) { "Package archive is too large" }
                                out.write(buffer, 0, count)
                            }
                        }
                    }
                    zip.closeEntry()
                }
            }
        }
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

private fun DataFramePage.toDataTable(title: String): DataTable? =
    value.toDataTable(maxRows = Int.MAX_VALUE)?.copy(
        title = title,
        totalRows = totalRows.coerceAtMost(Int.MAX_VALUE.toULong()).toInt(),
        rowOffset = offset.coerceAtMost(Int.MAX_VALUE.toULong()).toInt(),
    )

private fun writeCsvPage(
    writer: java.io.BufferedWriter,
    columns: List<String>,
    rows: List<List<String>>,
) {
    writeCsvHeader(writer, columns)
    writeCsvRows(writer, rows)
}

private fun writeCsvHeader(writer: java.io.BufferedWriter, columns: List<String>) {
    writer.write(columns.joinToString(",") { csvCell(it) })
    writer.newLine()
}

private fun writeCsvRows(writer: java.io.BufferedWriter, rows: List<List<String>>) {
    rows.forEach { row ->
        writer.write(row.joinToString(",") { csvCell(it) })
        writer.newLine()
    }
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

fun RValue.toDataTable(maxRows: Int = DATA_PAGE_SIZE, startRow: Int = 0): DataTable? {
    val classes = metadata.`class`?.filterNotNull().orEmpty()
    val isDataFrame = "data.frame" in classes
    val isMatrixLike = metadata.dim.orEmpty().size == 2 && kind != RValueKind.LIST

    if (isDataFrame && kind == RValueKind.LIST && listValues.isNotEmpty()) {
        val columns = metadata.names.orEmpty()
            .mapIndexed { index, name -> name?.takeIf { it.isNotBlank() } ?: "V${index + 1}" }
        val rowCount = listValues.maxOf { it.vectorLength() }
        val from = startRow.coerceIn(0, rowCount)
        val rows = (from until minOf(rowCount, from + maxRows)).map { row ->
            listValues.map { column -> column.valueAt(row) }
        }
        return DataTable(
            title = "data.frame ${rowCount}x${listValues.size}",
            columns = columns,
            rows = rows,
            totalRows = rowCount,
            rowOffset = from,
        )
    }

    if (isMatrixLike) {
        val dims = metadata.dim.orEmpty()
        val rowCount = dims[0]
        val colCount = dims[1]
        val columns = metadata.names.orEmpty().takeIf { it.size == colCount }
            ?.mapIndexed { index, name -> name?.takeIf { it.isNotBlank() } ?: "V${index + 1}" }
            ?: (1..colCount).map { "V$it" }
        val from = startRow.coerceIn(0, rowCount)
        val rows = (from until minOf(rowCount, from + maxRows)).map { row ->
            (0 until colCount).map { col -> valueAt(row + col * rowCount) }
        }
        return DataTable("matrix ${rowCount}x$colCount", columns, rows, rowCount, from)
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

private fun csvCell(value: String): String =
    if (value.any { it == ',' || it == '"' || it == '\n' }) "\"${value.replace("\"", "\"\"")}\"" else value

private fun String.sanitizeFileName(): String =
    replace(Regex("[^A-Za-z0-9._-]"), "_").ifBlank { "untitled.R" }

private fun RStudioUiState.toWorkspaceProject(): WorkspaceProject? {
    val name = projectName ?: return null
    val treeUri = projectTreeUri ?: return null
    val root = projectRoot ?: return null
    return WorkspaceProject(name, treeUri, root, projectFiles)
}

private fun List<EditorDocument>.upsert(document: EditorDocument): List<EditorDocument> =
    filterNot { it.id == document.id } + document

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
private const val DATA_PAGE_SIZE = 200
private const val EXPORT_PAGE_SIZE = 500UL
private const val MAX_PACKAGE_BYTES = 100L * 1024L * 1024L
