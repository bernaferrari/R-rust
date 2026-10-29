package com.rstudio.mobile.ui

import androidx.annotation.StringRes
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Help
import androidx.compose.material.icons.automirrored.filled.ListAlt
import androidx.compose.material.icons.filled.Code
import androidx.compose.material.icons.filled.Folder
import androidx.compose.material.icons.filled.InsertChart
import androidx.compose.material.icons.filled.TableRows
import androidx.compose.material.icons.filled.Terminal
import androidx.compose.material.icons.filled.Widgets
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.rstudio.mobile.R

internal enum class Destination(@StringRes val labelRes: Int, val icon: ImageVector) {
    Editor(R.string.navigation_editor, Icons.Default.Code),
    Console(R.string.navigation_console, Icons.Default.Terminal),
    Inspect(R.string.navigation_inspect, Icons.Default.Widgets),
    Files(R.string.navigation_files, Icons.Default.Folder),
}

internal enum class InspectTab(@StringRes val labelRes: Int, val icon: ImageVector) {
    Data(R.string.navigation_data, Icons.Default.TableRows),
    Plots(R.string.navigation_plots, Icons.Default.InsertChart),
    Environment(R.string.navigation_environment, Icons.AutoMirrored.Filled.ListAlt),
    Packages(R.string.navigation_packages, Icons.Default.Widgets),
    Help(R.string.navigation_help, Icons.AutoMirrored.Filled.Help),
}

@Composable
internal fun WorkbenchNavigationBar(selected: Destination, onSelect: (Destination) -> Unit) {
    NavigationBar {
        Destination.entries.forEach { item ->
            NavigationBarItem(
                selected = selected == item,
                onClick = { onSelect(item) },
                icon = { Icon(item.icon, contentDescription = null) },
                label = { Text(stringResource(item.labelRes)) },
            )
        }
    }
}

@Composable
internal fun InspectorTabs(selectedIndex: Int, onSelect: (Int) -> Unit) {
    ScrollableTabRow(selectedTabIndex = selectedIndex, edgePadding = 8.dp) {
        InspectTab.entries.forEachIndexed { index, item ->
            Tab(
                selected = selectedIndex == index,
                onClick = { onSelect(index) },
                icon = { Icon(item.icon, contentDescription = null) },
                text = { Text(stringResource(item.labelRes)) },
            )
        }
    }
}

@Composable
internal fun WorkspaceModeTabs(filesSelected: Boolean, onSelectFiles: (Boolean) -> Unit) {
    ScrollableTabRow(selectedTabIndex = if (filesSelected) 1 else 0, edgePadding = 8.dp) {
        Tab(selected = !filesSelected, onClick = { onSelectFiles(false) }, text = { Text(stringResource(R.string.navigation_inspect)) })
        Tab(selected = filesSelected, onClick = { onSelectFiles(true) }, text = { Text(stringResource(R.string.navigation_files)) })
    }
}
