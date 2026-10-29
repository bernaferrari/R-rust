package com.rstudio.mobile.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.rport.uniffi.RuntimeInfo
import com.rstudio.mobile.R

@Composable
internal fun RuntimeInformationDialog(info: RuntimeInfo?, onDismiss: () -> Unit, onReset: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.runtime_dialog_title), modifier = Modifier.semantics { heading() }) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                Text(stringResource(if (info?.isActive == true) R.string.runtime_active else R.string.runtime_unavailable))
                Text(
                    stringResource(R.string.runtime_temp_directory, info?.tempDir ?: stringResource(R.string.runtime_unknown_directory)),
                    style = MaterialTheme.typography.bodySmall,
                )
                Text(stringResource(R.string.runtime_library_paths), modifier = Modifier.padding(top = 8.dp))
                info?.libraryPaths?.forEach { path -> Text(path, style = MaterialTheme.typography.bodySmall) }
            }
        },
        confirmButton = {
            TextButton(onClick = onDismiss, modifier = Modifier.heightIn(min = 48.dp)) { Text(stringResource(R.string.runtime_close)) }
        },
        dismissButton = {
            TextButton(onClick = onReset, modifier = Modifier.heightIn(min = 48.dp)) { Text(stringResource(R.string.runtime_reset_session)) }
        },
    )
}

@Composable
internal fun DiscardChangesDialog(documentName: String, onDismiss: () -> Unit, onDiscard: () -> Unit) {
    val discardDescription = stringResource(R.string.document_discard_description, documentName)
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.document_discard_title), modifier = Modifier.semantics { heading() }) },
        text = { Text(stringResource(R.string.document_unsaved_message, documentName)) },
        confirmButton = {
            TextButton(
                onClick = onDiscard,
                modifier = Modifier.heightIn(min = 48.dp).semantics { contentDescription = discardDescription },
            ) { Text(stringResource(R.string.document_discard)) }
        },
        dismissButton = {
            TextButton(onClick = onDismiss, modifier = Modifier.heightIn(min = 48.dp)) { Text(stringResource(R.string.document_cancel)) }
        },
    )
}
