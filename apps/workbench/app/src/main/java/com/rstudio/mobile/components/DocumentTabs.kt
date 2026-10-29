package com.rstudio.mobile.components

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import com.rstudio.mobile.R
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
            val saveState = stringResource(
                if (document.isDirty) R.string.document_unsaved_changes else R.string.document_saved
            )
            Tab(
                selected = document.id == activeId,
                onClick = { onSelect(document.id) },
                text = {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(document.name, maxLines = 1)
                        if (document.isDirty) {
                            Text(" •", modifier = Modifier.clearAndSetSemantics {})
                        }
                        if (documents.size > 1) {
                            IconButton(
                                onClick = { onClose(document) },
                                modifier = Modifier.size(48.dp),
                            ) {
                                Icon(
                                    Icons.Default.Close,
                                    contentDescription = stringResource(R.string.document_close, document.name),
                                )
                            }
                        }
                    }
                },
                modifier = Modifier.padding(horizontal = 2.dp).semantics { stateDescription = saveState },
            )
        }
    }
}
