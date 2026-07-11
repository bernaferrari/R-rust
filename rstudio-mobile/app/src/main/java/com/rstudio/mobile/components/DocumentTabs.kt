package com.rstudio.mobile.components

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.rstudio.mobile.runtime.EditorDocument

@Composable
fun DocumentTabs(
    documents: List<EditorDocument>,
    activeId: String,
    onSelect: (String) -> Unit,
    onClose: (EditorDocument) -> Unit,
) {
    if (documents.isEmpty()) return
    val selected = documents.indexOfFirst { it.id == activeId }.coerceAtLeast(0)
    ScrollableTabRow(selectedTabIndex = selected, edgePadding = 4.dp) {
        documents.forEach { document ->
            Tab(
                selected = document.id == activeId,
                onClick = { onSelect(document.id) },
                text = {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(document.name + if (document.isDirty) " •" else "", maxLines = 1)
                        if (documents.size > 1) {
                            IconButton(onClick = { onClose(document) }) {
                                Icon(Icons.Default.Close, contentDescription = "Close ${document.name}")
                            }
                        }
                    }
                },
                modifier = Modifier.padding(horizontal = 2.dp),
            )
        }
    }
}
