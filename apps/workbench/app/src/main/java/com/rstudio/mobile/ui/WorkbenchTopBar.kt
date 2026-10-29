package com.rstudio.mobile.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.FolderOpen
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import com.rstudio.mobile.R

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun WorkbenchTopBar(
    projectName: String?,
    fileName: String,
    isDirty: Boolean,
    isRunning: Boolean,
    onOpenProject: () -> Unit,
    onRuntimeInfo: () -> Unit,
    onStop: () -> Unit,
    onRun: () -> Unit,
) {
    val saveState = stringResource(if (isDirty) R.string.document_unsaved_changes else R.string.document_saved)
    TopAppBar(
        title = {
            Column {
                Text(projectName ?: stringResource(R.string.workbench_title), maxLines = 1)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        fileName,
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        modifier = Modifier.semantics { stateDescription = saveState },
                    )
                    if (isDirty) {
                        Text(
                            " •",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.clearAndSetSemantics {},
                        )
                    }
                }
            }
        },
        actions = {
            IconButton(onClick = onOpenProject, enabled = !isRunning, modifier = Modifier.size(48.dp)) {
                Icon(Icons.Default.FolderOpen, contentDescription = stringResource(R.string.files_open_project_description))
            }
            IconButton(onClick = onRuntimeInfo, modifier = Modifier.size(48.dp)) {
                Icon(Icons.Default.Info, contentDescription = stringResource(R.string.workbench_runtime_information))
            }
            if (isRunning) {
                FilledIconButton(onClick = onStop, modifier = Modifier.size(48.dp)) {
                    Icon(Icons.Default.Stop, contentDescription = stringResource(R.string.workbench_stop_execution))
                }
            } else {
                FilledIconButton(onClick = onRun, modifier = Modifier.size(48.dp)) {
                    Icon(Icons.Default.PlayArrow, contentDescription = stringResource(R.string.workbench_run_script))
                }
            }
        },
    )
}
