package com.rstudio.mobile.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

private data class FileEntry(
    val path: String,
    val detail: String,
)

@Composable
fun FileBrowser() {
    val entries = listOf(
        FileEntry("/data/data/com.rstudio.mobile/files", "App-private working directory."),
        FileEntry("/sdcard/Download", "User-visible download location."),
        FileEntry("/sdcard/Documents", "Shared documents area."),
        FileEntry("./", "Project-relative path for scripted workflows."),
    )

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text("Files", style = MaterialTheme.typography.titleMedium)
        Text(
            "Baseline browser view for local files and project assets.",
            style = MaterialTheme.typography.bodyMedium,
        )

        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
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
