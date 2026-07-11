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
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.IconButton
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
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
    onRename: (ProjectFile, String) -> Unit,
    onDelete: (ProjectFile) -> Unit,
    onCreateFolder: (String) -> Unit,
    onExportProject: () -> Unit,
    onSaveWorkspace: () -> Unit = {},
    onLoadWorkspace: () -> Unit = {},
) {
    var createFolderOpen by remember { mutableStateOf(false) }
    var renameTarget by remember { mutableStateOf<ProjectFile?>(null) }
    var nameInput by remember { mutableStateOf("") }
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
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    OutlinedButton(onClick = onExportProject) { Text("Export") }
                    OutlinedButton(onClick = onCloseProject) { Text("Close") }
                }
            }
        }
        if (projectName != null) {
            OutlinedButton(onClick = { createFolderOpen = true }, modifier = Modifier.fillMaxWidth()) {
                Text("New folder")
            }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = onNewScript, modifier = Modifier.weight(1f)) { Text("New") }
            OutlinedButton(onClick = onOpenScript, modifier = Modifier.weight(1f)) { Text("Open file") }
            FilledTonalButton(onClick = onImportCsv, modifier = Modifier.weight(1f)) { Text("Import") }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = onSaveWorkspace, modifier = Modifier.weight(1f)) { Text("Save workspace") }
            OutlinedButton(onClick = onLoadWorkspace, modifier = Modifier.weight(1f)) { Text("Load workspace") }
        }
        LazyColumn(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            if (projectFiles.isNotEmpty()) {
                item { Text("Project", style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 4.dp)) }
                items(projectFiles, key = { it.uri }) { file ->
                    var menuOpen by remember(file.uri) { mutableStateOf(false) }
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
                        IconButton(onClick = { menuOpen = true }) {
                            Icon(Icons.Default.MoreVert, contentDescription = "Actions for ${file.name}")
                        }
                        DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                            DropdownMenuItem(
                                text = { Text("Rename") },
                                onClick = {
                                    menuOpen = false
                                    nameInput = file.name
                                    renameTarget = file
                                },
                            )
                            DropdownMenuItem(
                                text = { Text("Delete") },
                                onClick = { menuOpen = false; onDelete(file) },
                            )
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

    if (createFolderOpen) {
        AlertDialog(
            onDismissRequest = { createFolderOpen = false },
            title = { Text("New folder") },
            text = { OutlinedTextField(value = nameInput, onValueChange = { nameInput = it }, label = { Text("Folder name") }, singleLine = true) },
            confirmButton = {
                TextButton(onClick = { if (nameInput.isNotBlank()) { onCreateFolder(nameInput); createFolderOpen = false; nameInput = "" } }) { Text("Create") }
            },
            dismissButton = { TextButton(onClick = { createFolderOpen = false }) { Text("Cancel") } },
        )
    }
    renameTarget?.let { target ->
        AlertDialog(
            onDismissRequest = { renameTarget = null },
            title = { Text("Rename ${target.name}") },
            text = { OutlinedTextField(value = nameInput, onValueChange = { nameInput = it }, label = { Text("New name") }, singleLine = true) },
            confirmButton = {
                TextButton(onClick = { if (nameInput.isNotBlank()) { onRename(target, nameInput); renameTarget = null } }) { Text("Rename") }
            },
            dismissButton = { TextButton(onClick = { renameTarget = null }) { Text("Cancel") } },
        )
    }
}

private fun formatBytes(bytes: Long): String = when {
    bytes < 1_024 -> "$bytes B"
    bytes < 1_048_576 -> "${bytes / 1_024} KB"
    else -> "${bytes / 1_048_576} MB"
}
