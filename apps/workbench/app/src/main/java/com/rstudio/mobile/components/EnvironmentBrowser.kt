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
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.rstudio.mobile.R
import com.rstudio.mobile.runtime.EnvEntry

@Composable
fun EnvironmentBrowser(
    entries: List<EnvEntry>,
    onRefresh: () -> Unit,
    onOpen: (String) -> Unit,
    onRemove: (String) -> Unit,
) {
    var query by remember { mutableStateOf("") }
    val filtered = remember(entries, query) {
        entries.filter { query.isBlank() || it.name.contains(query, true) || it.kind.contains(query, true) }
    }
    Column(Modifier.fillMaxSize().padding(12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Column {
                Text(stringResource(R.string.environment_title), style = MaterialTheme.typography.titleMedium)
                Text(
                    pluralStringResource(R.plurals.environment_object_count, entries.size, entries.size),
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            FilledTonalButton(onClick = onRefresh) { Text(stringResource(R.string.environment_refresh)) }
        }
        OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            modifier = Modifier.fillMaxWidth(),
            label = { Text(stringResource(R.string.environment_search_label)) },
            singleLine = true,
        )
        if (filtered.isEmpty()) {
            Text(
                stringResource(if (entries.isEmpty()) R.string.environment_empty else R.string.environment_no_matches),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        } else {
            LazyColumn(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                items(filtered, key = { it.name }) { entry ->
                    val openLabel = stringResource(R.string.environment_open_object, entry.name)
                    val removeLabel = stringResource(R.string.environment_remove_object, entry.name)
                    ElevatedCard(
                        modifier = Modifier.fillMaxWidth().clickable(
                            onClickLabel = openLabel,
                            role = Role.Button,
                        ) { onOpen(entry.name) },
                    ) {
                        Column(Modifier.padding(horizontal = 14.dp, vertical = 10.dp)) {
                            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                                Text(entry.name, style = MaterialTheme.typography.titleSmall)
                                Text(entry.kind, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary)
                            }
                            Text(entry.summary, style = MaterialTheme.typography.bodySmall, maxLines = 2)
                            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                androidx.compose.material3.TextButton(onClick = { onOpen(entry.name) }) {
                                    Text(
                                        stringResource(R.string.environment_open),
                                        modifier = Modifier.semantics { contentDescription = openLabel },
                                    )
                                }
                                androidx.compose.material3.TextButton(onClick = { onRemove(entry.name) }) {
                                    Text(
                                        stringResource(R.string.environment_remove),
                                        modifier = Modifier.semantics { contentDescription = removeLabel },
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
