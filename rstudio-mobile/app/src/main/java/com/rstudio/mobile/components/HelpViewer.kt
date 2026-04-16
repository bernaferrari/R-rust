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
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

private data class HelpTopic(
    val title: String,
    val summary: String,
)

@Composable
fun HelpViewer() {
    var query by remember { mutableStateOf("") }
    val topics = listOf(
        HelpTopic("help()", "Open the topic index and package docs."),
        HelpTopic("plot()", "Render into the active graphics device."),
        HelpTopic("library()", "Load installed packages into the session."),
        HelpTopic("q()", "Exit the current desktop host session."),
        HelpTopic("serialize()", "Round-trip objects through the binary format."),
    )
    val filtered = topics.filter {
        query.isBlank() || it.title.contains(query, ignoreCase = true) || it.summary.contains(query, ignoreCase = true)
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text("Help", style = MaterialTheme.typography.titleMedium)
        Text(
            "Searchable quick reference for the runtime surface.",
            style = MaterialTheme.typography.bodyMedium,
        )

        OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("Search topics") },
            singleLine = true,
        )

        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            items(filtered) { topic ->
                ElevatedCard(modifier = Modifier.fillMaxWidth()) {
                    Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text(topic.title, style = MaterialTheme.typography.titleSmall)
                        Text(topic.summary, style = MaterialTheme.typography.bodySmall)
                    }
                }
            }
        }
    }
}
