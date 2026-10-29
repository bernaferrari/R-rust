package com.rstudio.mobile.components

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.heightIn
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import com.rstudio.mobile.R
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
    val openProjectDescription = stringResource(R.string.files_open_project_description)
    val exportDescription = stringResource(R.string.files_export_project_description, projectName.orEmpty())
    val closeDescription = stringResource(R.string.files_close_project_description, projectName.orEmpty())
    val newDescription = stringResource(R.string.files_new_script_description)
    val openDescription = stringResource(R.string.files_open_script_description)
    val importDescription = stringResource(R.string.files_import_description)
    Column(Modifier.fillMaxSize().padding(12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(projectName ?: stringResource(R.string.files_title), style = MaterialTheme.typography.titleMedium)
                Text(
                    projectRoot ?: stringResource(R.string.files_project_hint),
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            if (projectName == null) {
                FilledTonalButton(onClick = onOpenProject, modifier = Modifier.heightIn(min = 48.dp).semantics { contentDescription = openProjectDescription }) { Text(stringResource(R.string.files_open_folder)) }
            } else {
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    OutlinedButton(onClick = onExportProject, modifier = Modifier.heightIn(min = 48.dp).semantics { contentDescription = exportDescription }) { Text(stringResource(R.string.files_export)) }
                    OutlinedButton(onClick = onCloseProject, modifier = Modifier.heightIn(min = 48.dp).semantics { contentDescription = closeDescription }) { Text(stringResource(R.string.files_close)) }
                }
            }
        }
        if (projectName != null) {
            OutlinedButton(onClick = { createFolderOpen = true }, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)) {
                Text(stringResource(R.string.files_new_folder))
            }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = onNewScript, modifier = Modifier.weight(1f).heightIn(min = 48.dp).semantics { contentDescription = newDescription }) { Text(stringResource(R.string.files_new)) }
            OutlinedButton(onClick = onOpenScript, modifier = Modifier.weight(1f).heightIn(min = 48.dp).semantics { contentDescription = openDescription }) { Text(stringResource(R.string.files_open_file)) }
            FilledTonalButton(onClick = onImportCsv, modifier = Modifier.weight(1f).heightIn(min = 48.dp).semantics { contentDescription = importDescription }) { Text(stringResource(R.string.files_import)) }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = onSaveWorkspace, modifier = Modifier.weight(1f).heightIn(min = 48.dp)) { Text(stringResource(R.string.files_save_workspace)) }
            OutlinedButton(onClick = onLoadWorkspace, modifier = Modifier.weight(1f).heightIn(min = 48.dp)) { Text(stringResource(R.string.files_load_workspace)) }
        }
        LazyColumn(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            if (projectFiles.isNotEmpty()) {
                item { Text(stringResource(R.string.files_project_section), style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 4.dp)) }
                items(projectFiles, key = { it.uri }) { file ->
                    var menuOpen by remember(file.uri) { mutableStateOf(false) }
                    val fileDescription = stringResource(
                        if (file.isDirectory) R.string.files_folder_description else R.string.files_file_description,
                        file.relativePath,
                    )
                    val openAction = stringResource(R.string.files_open_action, file.relativePath)
                    val renameDescription = stringResource(R.string.files_rename_target, file.relativePath)
                    val deleteDescription = stringResource(R.string.files_delete_target, file.relativePath)
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .then(
                                if (file.isDirectory) Modifier.semantics(mergeDescendants = true) {}
                                else Modifier.clickable(onClickLabel = openAction, role = Role.Button) { onOpenProjectFile(file) }
                            )
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
                            Text(file.name, style = MaterialTheme.typography.bodyMedium, maxLines = 1,
                                modifier = Modifier.semantics { contentDescription = fileDescription })
                            if (!file.isDirectory) Text(formatBytes(file.size), style = MaterialTheme.typography.labelSmall)
                        }
                        IconButton(onClick = { menuOpen = true }, modifier = Modifier.size(48.dp)) {
                            Icon(Icons.Default.MoreVert, contentDescription = stringResource(R.string.files_actions_target, file.relativePath))
                        }
                        DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                            DropdownMenuItem(
                                text = { Text(stringResource(R.string.files_rename)) },
                                modifier = Modifier.semantics { contentDescription = renameDescription },
                                onClick = {
                                    menuOpen = false
                                    nameInput = file.name
                                    renameTarget = file
                                },
                            )
                            DropdownMenuItem(
                                text = { Text(stringResource(R.string.files_delete)) },
                                modifier = Modifier.semantics { contentDescription = deleteDescription },
                                onClick = { menuOpen = false; onDelete(file) },
                            )
                        }
                    }
                }
            }
            if (recentScripts.isNotEmpty()) {
                item { Text(stringResource(R.string.files_recent_section), style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 10.dp)) }
                items(recentScripts, key = { it.path }) { script ->
                    val recentDescription = stringResource(R.string.files_recent_description, script.path)
                    ElevatedCard(modifier = Modifier.fillMaxWidth().semantics { contentDescription = recentDescription }, onClick = { onOpenRecent(script.path) }) {
                        Column(Modifier.padding(12.dp)) {
                            Text(script.name, style = MaterialTheme.typography.titleSmall)
                            Text(script.path, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    }
                }
            }
            importedPath?.let { path ->
                item {
                    Text(stringResource(R.string.files_imported_section), style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 10.dp))
                    Text(path, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
            if (projectFiles.isEmpty() && recentScripts.isEmpty()) {
                item {
                    Text(
                        stringResource(R.string.files_empty_hint),
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
            title = { Text(stringResource(R.string.files_new_folder)) },
            text = { OutlinedTextField(value = nameInput, onValueChange = { nameInput = it }, label = { Text(stringResource(R.string.files_folder_name)) }, singleLine = true) },
            confirmButton = {
                TextButton(onClick = { if (nameInput.isNotBlank()) { onCreateFolder(nameInput); createFolderOpen = false; nameInput = "" } }) { Text(stringResource(R.string.files_create)) }
            },
            dismissButton = { TextButton(onClick = { createFolderOpen = false }) { Text(stringResource(R.string.files_cancel)) } },
        )
    }
    renameTarget?.let { target ->
        AlertDialog(
            onDismissRequest = { renameTarget = null },
            title = { Text(stringResource(R.string.files_rename_target, target.relativePath)) },
            text = { OutlinedTextField(value = nameInput, onValueChange = { nameInput = it }, label = { Text(stringResource(R.string.files_new_name)) }, singleLine = true) },
            confirmButton = {
                TextButton(onClick = { if (nameInput.isNotBlank()) { onRename(target, nameInput); renameTarget = null } }) { Text(stringResource(R.string.files_rename)) }
            },
            dismissButton = { TextButton(onClick = { renameTarget = null }) { Text(stringResource(R.string.files_cancel)) } },
        )
    }
}

private fun formatBytes(bytes: Long): String = when {
    bytes < 1_024 -> "$bytes B"
    bytes < 1_048_576 -> "${bytes / 1_024} KB"
    else -> "${bytes / 1_048_576} MB"
}
