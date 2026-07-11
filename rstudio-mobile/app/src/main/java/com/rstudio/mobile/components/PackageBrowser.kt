package com.rstudio.mobile.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.FilledTonalButton
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
import com.rport.uniffi.PackageInfo

@Composable
fun PackageBrowser(
    packages: List<PackageInfo>,
    loaded: Set<String>,
    onRefresh: () -> Unit,
    onLoad: (String) -> Unit,
) {
    var query by remember { mutableStateOf("") }
    val filtered = remember(packages, query) {
        packages.filter { query.isBlank() || it.name.contains(query, true) || it.title.contains(query, true) }
    }
    Column(Modifier.fillMaxSize().padding(12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Column {
                Text("Packages", style = MaterialTheme.typography.titleMedium)
                Text("Pure-R packages supported", style = MaterialTheme.typography.bodySmall)
            }
            FilledTonalButton(onClick = onRefresh) { Text("Refresh") }
        }
        OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("Find a package") },
            singleLine = true,
        )
        if (filtered.isEmpty()) {
            Text("No installed pure-R packages were found.", color = MaterialTheme.colorScheme.onSurfaceVariant)
        } else {
            LazyColumn(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                items(filtered, key = { it.name }) { pkg ->
                    ElevatedCard(Modifier.fillMaxWidth()) {
                        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                                Column(Modifier.weight(1f)) {
                                    Text(pkg.name, style = MaterialTheme.typography.titleSmall)
                                    Text(pkg.version, style = MaterialTheme.typography.labelMedium)
                                }
                                FilledTonalButton(onClick = { onLoad(pkg.name) }, enabled = pkg.name !in loaded) {
                                    Text(if (pkg.name in loaded) "Loaded" else "Load")
                                }
                            }
                            if (pkg.title.isNotBlank()) Text(pkg.title, style = MaterialTheme.typography.bodySmall)
                            if (pkg.needsCompilation) {
                                Text("Requires native compilation and may not run on Android.", color = MaterialTheme.colorScheme.error)
                            }
                        }
                    }
                }
            }
        }
    }
}
