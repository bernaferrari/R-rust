package com.rstudio.mobile.ui

import android.app.Activity
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Tab
import androidx.compose.material3.TabRow
import androidx.compose.material3.Text
import androidx.compose.material3.VerticalDivider
import androidx.compose.material3.windowsizeclass.ExperimentalMaterial3WindowSizeClassApi
import androidx.compose.material3.windowsizeclass.WindowWidthSizeClass
import androidx.compose.material3.windowsizeclass.calculateWindowSizeClass
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.material.icons.automirrored.filled.Help
import androidx.compose.material.icons.automirrored.filled.ListAlt
import com.rstudio.mobile.components.ConsoleView
import com.rstudio.mobile.components.EnvironmentBrowser
import com.rstudio.mobile.components.FileBrowser
import com.rstudio.mobile.components.HelpViewer
import com.rstudio.mobile.components.PlotView
import com.rstudio.mobile.components.ScriptEditor
import com.rstudio.mobile.runtime.RStudioRuntime
import com.rstudio.mobile.runtime.RStudioUiState
import kotlinx.coroutines.launch
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Code
import androidx.compose.material.icons.filled.Terminal
import androidx.compose.material.icons.filled.InsertChart
import androidx.compose.material.icons.filled.Folder

@OptIn(ExperimentalMaterial3WindowSizeClassApi::class)
@Composable
fun RStudioApp() {
    val activity = LocalContext.current as Activity
    val runtime = remember { RStudioRuntime(activity.applicationContext) }
    val state by runtime.state.collectAsState()
    val csvPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri: Uri? ->
        if (uri != null) runtime.importCsv(uri)
    }

    DisposableEffect(runtime) {
        onDispose { runtime.close() }
    }

    val windowSizeClass = calculateWindowSizeClass(activity)
    val isTablet = windowSizeClass.widthSizeClass == WindowWidthSizeClass.Expanded

    if (isTablet) {
        TabletLayout(
            state = state,
            runtime = runtime,
            onImportCsv = { csvPicker.launch(arrayOf("text/*", "text/csv", "application/csv")) },
        )
    } else {
        PhoneLayout(
            state = state,
            runtime = runtime,
            onImportCsv = { csvPicker.launch(arrayOf("text/*", "text/csv", "application/csv")) },
        )
    }
}

@Composable
private fun PhoneLayout(
    state: RStudioUiState,
    runtime: RStudioRuntime,
    onImportCsv: () -> Unit,
) {
    val pagerState = rememberPagerState(pageCount = { 6 })
    val scope = rememberCoroutineScope()
    val tabs = listOf(
        "Script" to Icons.Default.Code,
        "Console" to Icons.Default.Terminal,
        "Plots" to Icons.Default.InsertChart,
        "Env" to Icons.AutoMirrored.Filled.ListAlt,
        "Files" to Icons.Default.Folder,
        "Help" to Icons.AutoMirrored.Filled.Help
    )

    Column(Modifier.fillMaxSize()) {
        TabRow(selectedTabIndex = pagerState.currentPage) {
            tabs.forEachIndexed { index, (title, icon) ->
                Tab(
                    selected = pagerState.currentPage == index,
                    onClick = { scope.launch { pagerState.animateScrollToPage(index) } },
                    icon = { Icon(icon, contentDescription = title) },
                    text = { Text(title, maxLines = 1) }
                )
            }
        }

        HorizontalPager(state = pagerState, Modifier.fillMaxSize()) { page ->
            when (page) {
                0 -> ScriptEditor(
                    code = state.code,
                    fileName = state.currentFileName,
                    isRunning = state.isRunning,
                    status = state.status,
                    onCodeChange = runtime::updateCode,
                    onRun = runtime::runCurrentCode,
                    onRenderPlot = runtime::renderCurrentCode,
                    onImportCsv = onImportCsv,
                )
                1 -> ConsoleView(
                    console = state.console,
                    lastValueSummary = state.lastValueSummary,
                    isRunning = state.isRunning,
                    status = state.status,
                    onClear = runtime::clearConsole,
                    onCancel = runtime::cancel,
                )
                2 -> PlotView(plot = state.lastPlot, isRunning = state.isRunning, onRender = runtime::renderCurrentCode)
                3 -> EnvironmentBrowser(entries = state.environment, onRefresh = runtime::refreshEnvironment)
                4 -> FileBrowser(importedPath = state.importedPath, onImportCsv = onImportCsv)
                5 -> HelpViewer()
            }
        }
    }
}

@Composable
private fun TabletLayout(
    state: RStudioUiState,
    runtime: RStudioRuntime,
    onImportCsv: () -> Unit,
) {
    Row(Modifier.fillMaxSize()) {
        Column(Modifier.weight(1f)) {
            Box(Modifier.weight(1f).fillMaxWidth()) {
                ScriptEditor(
                    code = state.code,
                    fileName = state.currentFileName,
                    isRunning = state.isRunning,
                    status = state.status,
                    onCodeChange = runtime::updateCode,
                    onRun = runtime::runCurrentCode,
                    onRenderPlot = runtime::renderCurrentCode,
                    onImportCsv = onImportCsv,
                )
            }
            HorizontalDivider()
            Box(Modifier.fillMaxWidth().weight(0.6f)) {
                ConsoleView(
                    console = state.console,
                    lastValueSummary = state.lastValueSummary,
                    isRunning = state.isRunning,
                    status = state.status,
                    onClear = runtime::clearConsole,
                    onCancel = runtime::cancel,
                )
            }
        }

        VerticalDivider()

        Column(Modifier.width(320.dp).fillMaxHeight()) {
            val rightPagerState = rememberPagerState(pageCount = { 4 })
            val scope = rememberCoroutineScope()
            val tabs = listOf(
                "Plots" to Icons.Default.InsertChart,
                "Env" to Icons.AutoMirrored.Filled.ListAlt,
                "Files" to Icons.Default.Folder,
                "Help" to Icons.AutoMirrored.Filled.Help
            )

            TabRow(selectedTabIndex = rightPagerState.currentPage) {
                tabs.forEachIndexed { index, (title, icon) ->
                    Tab(
                        selected = rightPagerState.currentPage == index,
                        onClick = { scope.launch { rightPagerState.animateScrollToPage(index) } },
                        icon = { Icon(icon, contentDescription = title) }
                    )
                }
            }

            HorizontalPager(state = rightPagerState, Modifier.fillMaxSize()) { page ->
                when (page) {
                    0 -> PlotView(plot = state.lastPlot, isRunning = state.isRunning, onRender = runtime::renderCurrentCode)
                    1 -> EnvironmentBrowser(entries = state.environment, onRefresh = runtime::refreshEnvironment)
                    2 -> FileBrowser(importedPath = state.importedPath, onImportCsv = onImportCsv)
                    3 -> HelpViewer()
                }
            }
        }
    }
}
