package com.rstudio.mobile.components

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Description
import androidx.compose.material.icons.filled.Folder
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.rstudio.mobile.data.ProjectFile
import com.rstudio.mobile.runtime.ScriptFile

@Composable
fun FileBrowser(
    projectName: String?,
    projectRoot: String?,
    projectFiles: List<ProjectFile>,
    importedPath: String?,
    recentScripts: List<ScriptFile>,
    onOpenProject: () -> Unit,
    onCloseProject: () -> Unit,
    onImportCsv: () -> Unit,
    onOpenScript: () -> Unit,
    onNewScript: () -> Unit,
    onOpenRecent: (String) -> Unit,
    onOpenProjectFile: (ProjectFile) -> Unit,
) {
    Column(Modifier.fillMaxSize().padding(12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(projectName ?: "Files", style = MaterialTheme.typography.titleMedium)
                Text(
                    projectRoot ?: "Choose an Android folder to create a working project.",
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            if (projectName == null) {
                FilledTonalButton(onClick = onOpenProject) { Text("Open folder") }
            } else {
                OutlinedButton(onClick = onCloseProject) { Text("Close") }
            }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = onNewScript, modifier = Modifier.weight(1f)) { Text("New") }
            OutlinedButton(onClick = onOpenScript, modifier = Modifier.weight(1f)) { Text("Open file") }
            FilledTonalButton(onClick = onImportCsv, modifier = Modifier.weight(1f)) { Text("Import") }
        }
        LazyColumn(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            if (projectFiles.isNotEmpty()) {
                item { Text("Project", style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 4.dp)) }
                items(projectFiles, key = { it.uri }) { file ->
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable(enabled = !file.isDirectory) { onOpenProjectFile(file) }
                            .padding(start = (file.relativePath.count { it == '/' } * 16).dp, top = 8.dp, bottom = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        Icon(
                            if (file.isDirectory) Icons.Default.Folder else Icons.Default.Description,
                            contentDescription = null,
                            tint = if (file.isDirectory) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Column(Modifier.weight(1f)) {
                            Text(file.name, style = MaterialTheme.typography.bodyMedium, maxLines = 1)
                            if (!file.isDirectory) Text(formatBytes(file.size), style = MaterialTheme.typography.labelSmall)
                        }
                    }
                }
            }
            if (recentScripts.isNotEmpty()) {
                item { Text("Recent scripts", style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 10.dp)) }
                items(recentScripts, key = { it.path }) { script ->
                    ElevatedCard(modifier = Modifier.fillMaxWidth(), onClick = { onOpenRecent(script.path) }) {
                        Column(Modifier.padding(12.dp)) {
                            Text(script.name, style = MaterialTheme.typography.titleSmall)
                            Text(script.path, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    }
                }
            }
            importedPath?.let { path ->
                item {
                    Text("Last imported data", style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 10.dp))
                    Text(path, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
            if (projectFiles.isEmpty() && recentScripts.isEmpty()) {
                item {
                    Text(
                        "Open a project folder, an R script, or import a CSV/TSV file to begin.",
                        modifier = Modifier.padding(vertical = 18.dp),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

private fun formatBytes(bytes: Long): String = when {
    bytes < 1_024 -> "$bytes B"
    bytes < 1_048_576 -> "${bytes / 1_024} KB"
    else -> "${bytes / 1_048_576} MB"
}
