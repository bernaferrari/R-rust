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

private data class EnvEntry(
    val name: String,
    val value: String,
    val note: String,
)

@Composable
fun EnvironmentBrowser() {
    val entries = listOf(
        EnvEntry("Global environment", "R_GlobalEnv", "Top-level session state lives here."),
        EnvEntry("Base environment", "base", "Built-in functions and primitives."),
        EnvEntry("Empty environment", "R_EmptyEnv", "Root of the environment chain."),
        EnvEntry("Graphics device", "active", "Backed by the current render surface."),
        EnvEntry("Bindings", "rport::uniffi", "Generated Android-safe bridge surface."),
    )

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text("Environment", style = MaterialTheme.typography.titleMedium)
        Text(
            "Session state snapshot for the desktop/mobile parity baseline.",
            style = MaterialTheme.typography.bodyMedium,
        )

        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            items(entries) { entry ->
                ElevatedCard(modifier = Modifier.fillMaxWidth()) {
                    Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text(entry.name, style = MaterialTheme.typography.titleSmall)
                        Text(entry.value, style = MaterialTheme.typography.bodyMedium)
                        Text(entry.note, style = MaterialTheme.typography.bodySmall)
                    }
                }
            }
        }
    }
}
