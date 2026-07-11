package com.rstudio.mobile.ui

import android.app.Activity
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Help
import androidx.compose.material.icons.automirrored.filled.ListAlt
import androidx.compose.material.icons.filled.Code
import androidx.compose.material.icons.filled.Folder
import androidx.compose.material.icons.filled.FolderOpen
import androidx.compose.material.icons.filled.InsertChart
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material.icons.filled.TableRows
import androidx.compose.material.icons.filled.Terminal
import androidx.compose.material.icons.filled.Widgets
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.VerticalDivider
import androidx.compose.material3.windowsizeclass.ExperimentalMaterial3WindowSizeClassApi
import androidx.compose.material3.windowsizeclass.WindowWidthSizeClass
import androidx.compose.material3.windowsizeclass.calculateWindowSizeClass
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.rstudio.mobile.components.ConsoleView
import com.rstudio.mobile.components.DataTableView
import com.rstudio.mobile.components.EnvironmentBrowser
import com.rstudio.mobile.components.FileBrowser
import com.rstudio.mobile.components.HelpViewer
import com.rstudio.mobile.components.PackageBrowser
import com.rstudio.mobile.components.PlotView
import com.rstudio.mobile.components.ScriptEditor
import com.rstudio.mobile.runtime.RStudioRuntime
import com.rstudio.mobile.runtime.RStudioRuntimeFactory
import com.rstudio.mobile.runtime.RStudioUiState

private enum class Destination(val label: String, val icon: ImageVector) {
    Editor("Editor", Icons.Default.Code),
    Console("Console", Icons.Default.Terminal),
    Inspect("Inspect", Icons.Default.Widgets),
    Files("Files", Icons.Default.Folder),
}

private enum class InspectTab(val label: String, val icon: ImageVector) {
    Data("Data", Icons.Default.TableRows),
    Plots("Plots", Icons.Default.InsertChart),
    Environment("Environment", Icons.AutoMirrored.Filled.ListAlt),
    Packages("Packages", Icons.Default.Widgets),
    Help("Help", Icons.AutoMirrored.Filled.Help),
}

@OptIn(ExperimentalMaterial3WindowSizeClassApi::class, ExperimentalMaterial3Api::class)
@Composable
fun RStudioApp() {
    val activity = LocalContext.current as Activity
    val runtime: RStudioRuntime = viewModel(factory = RStudioRuntimeFactory(activity.application))
    val state by runtime.state.collectAsState()
    val csvPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri: Uri? ->
        if (uri != null) runtime.importCsv(uri)
    }
    val scriptPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri: Uri? ->
        if (uri != null) runtime.openScript(uri)
    }
    val projectPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri: Uri? ->
        if (uri != null) runtime.openProject(uri)
    }
    val scriptExporter = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("text/x-r-source")) { uri: Uri? ->
        if (uri != null) runtime.saveScriptTo(uri)
    }
    val plotExporter = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("image/png")) { uri: Uri? ->
        if (uri != null) runtime.savePlotTo(uri)
    }
    val actions = WorkbenchActions(
        openProject = { projectPicker.launch(null) },
        importData = {
            csvPicker.launch(
                arrayOf("text/csv", "text/tab-separated-values", "text/plain", "application/octet-stream")
            )
        },
        openScript = { scriptPicker.launch(arrayOf("text/x-r-source", "text/plain", "application/octet-stream")) },
        exportScript = { scriptExporter.launch(state.currentFileName) },
        exportPlot = { plotExporter.launch("Rplot-${state.plots.size.coerceAtLeast(1)}.png") },
    )
    val expanded = calculateWindowSizeClass(activity).widthSizeClass == WindowWidthSizeClass.Expanded
    var destination by rememberSaveable { mutableStateOf(Destination.Editor) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text(state.projectName ?: "R Workbench", maxLines = 1)
                        Text(
                            state.currentFileName + if (state.isDirty) " •" else "",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                        )
                    }
                },
                actions = {
                    IconButton(onClick = actions.openProject, enabled = !state.isRunning) {
                        Icon(Icons.Default.FolderOpen, contentDescription = "Open project folder")
                    }
                    if (state.isRunning) {
                        FilledIconButton(onClick = runtime::cancel) {
                            Icon(Icons.Default.Stop, contentDescription = "Stop R execution")
                        }
                    } else {
                        FilledIconButton(onClick = runtime::runCurrentCode) {
                            Icon(Icons.Default.PlayArrow, contentDescription = "Run entire script")
                        }
                    }
                },
            )
        },
        bottomBar = {
            if (!expanded) {
                NavigationBar {
                    Destination.entries.forEach { item ->
                        NavigationBarItem(
                            selected = destination == item,
                            onClick = { destination = item },
                            icon = { Icon(item.icon, contentDescription = null) },
                            label = { Text(item.label) },
                        )
                    }
                }
            }
        },
    ) { padding ->
        if (expanded) {
            ExpandedWorkspace(state, runtime, actions, Modifier.padding(padding))
        } else {
            CompactWorkspace(state, runtime, actions, destination, Modifier.padding(padding))
        }
    }
}

private data class WorkbenchActions(
    val openProject: () -> Unit,
    val importData: () -> Unit,
    val openScript: () -> Unit,
    val exportScript: () -> Unit,
    val exportPlot: () -> Unit,
)

@Composable
private fun CompactWorkspace(
    state: RStudioUiState,
    runtime: RStudioRuntime,
    actions: WorkbenchActions,
    selected: Destination,
    modifier: Modifier,
) {
    Box(modifier.fillMaxSize()) {
        when (selected) {
            Destination.Editor -> EditorPane(state, runtime, actions)
            Destination.Console -> ConsolePane(state, runtime)
            Destination.Inspect -> InspectPane(state, runtime, actions.exportPlot)
            Destination.Files -> FilesPane(state, runtime, actions)
        }
    }
}

@Composable
private fun ExpandedWorkspace(
    state: RStudioUiState,
    runtime: RStudioRuntime,
    actions: WorkbenchActions,
    modifier: Modifier,
) {
    Row(modifier.fillMaxSize()) {
        Column(Modifier.weight(1f)) {
            Box(Modifier.weight(1f).fillMaxWidth()) { EditorPane(state, runtime, actions) }
            HorizontalDivider()
            Box(Modifier.weight(0.62f).fillMaxWidth()) { ConsolePane(state, runtime) }
        }
        VerticalDivider()
        Column(Modifier.width(390.dp).fillMaxHeight()) {
            var filesSelected by rememberSaveable { mutableStateOf(false) }
            ScrollableTabRow(selectedTabIndex = if (filesSelected) 1 else 0, edgePadding = 8.dp) {
                Tab(selected = !filesSelected, onClick = { filesSelected = false }, text = { Text("Inspect") })
                Tab(selected = filesSelected, onClick = { filesSelected = true }, text = { Text("Files") })
            }
            if (filesSelected) FilesPane(state, runtime, actions) else InspectPane(state, runtime, actions.exportPlot)
        }
    }
}

@Composable
private fun EditorPane(state: RStudioUiState, runtime: RStudioRuntime, actions: WorkbenchActions) {
    ScriptEditor(
        code = state.code,
        fileName = state.currentFileName,
        isDirty = state.isDirty,
        isRunning = state.isRunning,
        status = state.status,
        onCodeChange = runtime::updateCode,
        onRunCode = runtime::runCode,
        onRunFile = runtime::runCurrentCode,
        onRenderPlot = runtime::renderCurrentCode,
        onImportCsv = actions.importData,
        onOpenScript = actions.openScript,
        onSaveScript = runtime::saveScriptLocal,
        onExportScript = actions.exportScript,
    )
}

@Composable
private fun ConsolePane(state: RStudioUiState, runtime: RStudioRuntime) {
    ConsoleView(
        console = state.console,
        history = state.consoleHistory,
        lastValueSummary = state.lastValueSummary,
        errorMessage = state.errorMessage,
        isRunning = state.isRunning,
        status = state.status,
        onEvaluate = runtime::evaluateConsole,
        onClear = runtime::clearConsole,
        onCancel = runtime::cancel,
    )
}

@Composable
private fun InspectPane(state: RStudioUiState, runtime: RStudioRuntime, onExportPlot: () -> Unit) {
    var tab by rememberSaveable { mutableIntStateOf(0) }
    Column(Modifier.fillMaxSize()) {
        ScrollableTabRow(selectedTabIndex = tab, edgePadding = 8.dp) {
            InspectTab.entries.forEachIndexed { index, item ->
                Tab(
                    selected = tab == index,
                    onClick = { tab = index },
                    icon = { Icon(item.icon, contentDescription = null) },
                    text = { Text(item.label) },
                )
            }
        }
        Box(Modifier.weight(1f).fillMaxWidth()) {
            when (InspectTab.entries[tab]) {
                InspectTab.Data -> DataTableView(state.dataTable)
                InspectTab.Plots -> PlotView(
                    plot = state.lastPlot,
                    plots = state.plots,
                    isRunning = state.isRunning,
                    onRender = runtime::renderCurrentCode,
                    onSelect = runtime::selectPlot,
                    onExport = onExportPlot,
                    onShare = runtime::sharePlot,
                )
                InspectTab.Environment -> EnvironmentBrowser(
                    entries = state.environment,
                    onRefresh = runtime::refreshEnvironment,
                    onOpen = runtime::inspectEnvironment,
                )
                InspectTab.Packages -> PackageBrowser(
                    packages = state.packages,
                    loaded = state.loadedPackages,
                    onRefresh = runtime::refreshPackages,
                    onLoad = runtime::loadPackage,
                )
                InspectTab.Help -> HelpViewer(
                    helpResult = state.helpResult,
                    helpLoading = state.helpLoading,
                    onLookupHelp = runtime::evaluateHelp,
                    onClearHelp = runtime::clearHelpResult,
                )
            }
        }
    }
}

@Composable
private fun FilesPane(state: RStudioUiState, runtime: RStudioRuntime, actions: WorkbenchActions) {
    FileBrowser(
        projectName = state.projectName,
        projectRoot = state.projectRoot,
        projectFiles = state.projectFiles,
        importedPath = state.importedPath,
        recentScripts = state.recentScripts,
        onOpenProject = actions.openProject,
        onCloseProject = runtime::closeProject,
        onImportCsv = actions.importData,
        onOpenScript = actions.openScript,
        onNewScript = runtime::newScript,
        onOpenRecent = runtime::openRecentScript,
        onOpenProjectFile = runtime::openProjectFile,
    )
}
