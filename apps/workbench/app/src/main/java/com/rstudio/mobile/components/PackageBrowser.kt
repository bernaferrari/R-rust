package com.rstudio.mobile.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.heightIn
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import com.rstudio.mobile.R
import com.rport.uniffi.PackageInfo

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun PackageBrowser(
    packages: List<PackageInfo>,
    loaded: Set<String>,
    onRefresh: () -> Unit,
    onLoad: (String) -> Unit,
    onInstall: () -> Unit = {},
    onRemove: (PackageInfo) -> Unit = {},
) {
    var query by remember { mutableStateOf("") }
    val filtered = remember(packages, query) {
        packages.filter { query.isBlank() || it.name.contains(query, true) || it.title.contains(query, true) }
    }
    Column(Modifier.fillMaxSize().padding(12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Column {
                Text(stringResource(R.string.packages_title), style = MaterialTheme.typography.titleMedium)
                Text(stringResource(R.string.packages_subtitle), style = MaterialTheme.typography.bodySmall)
            }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                FilledTonalButton(onClick = onInstall, modifier = Modifier.heightIn(min = 48.dp)) { Text(stringResource(R.string.packages_install_zip)) }
                FilledTonalButton(onClick = onRefresh, modifier = Modifier.heightIn(min = 48.dp)) { Text(stringResource(R.string.packages_refresh)) }
            }
        }
        OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            modifier = Modifier.fillMaxWidth(),
            label = { Text(stringResource(R.string.packages_search)) },
            singleLine = true,
        )
        if (filtered.isEmpty()) {
            Text(stringResource(R.string.packages_empty), color = MaterialTheme.colorScheme.onSurfaceVariant)
        } else {
            LazyColumn(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                items(filtered, key = { it.name }) { pkg ->
                    val isLoaded = pkg.name in loaded
                    val loadDescription = stringResource(
                        if (isLoaded) R.string.packages_loaded_description else R.string.packages_load_description,
                        pkg.name,
                    )
                    val removeDescription = stringResource(R.string.packages_remove_description, pkg.name)
                    ElevatedCard(Modifier.fillMaxWidth()) {
                        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                            FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                Column {
                                    Text(pkg.name, style = MaterialTheme.typography.titleSmall)
                                    Text(pkg.version, style = MaterialTheme.typography.labelMedium)
                                }
                                FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                                    FilledTonalButton(
                                        onClick = { onLoad(pkg.name) },
                                        enabled = !isLoaded,
                                        modifier = Modifier.heightIn(min = 48.dp).semantics { contentDescription = loadDescription },
                                    ) {
                                        Text(stringResource(if (isLoaded) R.string.packages_loaded else R.string.packages_load))
                                    }
                                    if (pkg.libraryPath.contains("/R/library")) {
                                        FilledTonalButton(
                                            onClick = { onRemove(pkg) },
                                            modifier = Modifier.heightIn(min = 48.dp).semantics { contentDescription = removeDescription },
                                        ) { Text(stringResource(R.string.packages_remove)) }
                                    }
                                }
                            }
                            if (pkg.title.isNotBlank()) Text(pkg.title, style = MaterialTheme.typography.bodySmall)
                            if (pkg.needsCompilation) {
                                Text(stringResource(R.string.packages_native_warning), color = MaterialTheme.colorScheme.error)
                            }
                        }
                    }
                }
            }
        }
    }
}
