package com.rstudio.mobile.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.rstudio.mobile.runtime.ScriptFile

private data class FileEntry(
    val path: String,
    val detail: String,
)

@Composable
fun FileBrowser(
    importedPath: String?,
    recentScripts: List<ScriptFile>,
    onImportCsv: () -> Unit,
    onOpenScript: () -> Unit,
    onNewScript: () -> Unit,
    onOpenRecent: (String) -> Unit,
) {
    val entries = listOf(
        FileEntry("App files", "R sees copied/imported files through the app-private workspace."),
        FileEntry("CSV import", importedPath ?: "Choose a CSV from Android files to import it into R."),
        FileEntry("Temporary files", "tempdir() and tempfile() resolve inside Android cache."),
    )

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        androidx.compose.foundation.layout.Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text("Files", style = MaterialTheme.typography.titleMedium)
            androidx.compose.foundation.layout.Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(onClick = onNewScript) { Text("New") }
                OutlinedButton(onClick = onOpenScript) { Text("Open") }
                FilledTonalButton(onClick = onImportCsv) { Text("CSV") }
            }
        }

        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            if (recentScripts.isNotEmpty()) {
                item {
                    Text("Recent scripts", style = MaterialTheme.typography.titleSmall)
                }
            }
            items(recentScripts) { script ->
                ElevatedCard(modifier = Modifier.fillMaxWidth(), onClick = { onOpenRecent(script.path) }) {
                    Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text(script.name, style = MaterialTheme.typography.titleSmall)
                        Text(script.path, style = MaterialTheme.typography.bodySmall)
                    }
                }
            }
            items(entries) { entry ->
                ElevatedCard(modifier = Modifier.fillMaxWidth()) {
                    Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text(entry.path, style = MaterialTheme.typography.titleSmall)
                        Text(entry.detail, style = MaterialTheme.typography.bodySmall)
                    }
                }
            }
        }
    }
}
