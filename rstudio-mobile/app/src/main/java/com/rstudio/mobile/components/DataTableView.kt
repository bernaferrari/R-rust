package com.rstudio.mobile.components

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.TableRows
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.rstudio.mobile.runtime.DataTable

@Composable
fun DataTableView(table: DataTable?) {
    if (table == null) {
        Column(
            modifier = Modifier.fillMaxSize().padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Icon(Icons.Default.TableRows, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
            Text("No table result", style = MaterialTheme.typography.titleMedium)
            Text(
                "Import a CSV or evaluate a data.frame to inspect rows and columns here.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        return
    }

    val horizontal = rememberScrollState()
    var query by remember(table.title) { mutableStateOf("") }
    var sortColumn by remember(table.title) { mutableIntStateOf(-1) }
    var ascending by remember(table.title) { mutableStateOf(true) }
    val visibleRows = remember(table, query, sortColumn, ascending) {
        val filtered = table.rows.filter { row -> query.isBlank() || row.any { it.contains(query, ignoreCase = true) } }
        if (sortColumn !in table.columns.indices) filtered else filtered.sortedWith { left, right ->
            val comparison = compareCells(left.getOrElse(sortColumn) { "" }, right.getOrElse(sortColumn) { "" })
            if (ascending) comparison else -comparison
        }
    }
    Column(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxWidth().padding(12.dp)) {
            Text(table.title, style = MaterialTheme.typography.titleMedium)
            val shown = table.rows.size
            Text(
                if (shown < table.totalRows) "Loaded $shown of ${table.totalRows} rows" else "${table.totalRows} rows",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                label = { Text("Filter loaded rows") },
                singleLine = true,
            )
        }
        HorizontalDivider()
        Column(Modifier.fillMaxSize().horizontalScroll(horizontal)) {
            TableRow(
                cells = listOf("#") + table.columns,
                header = true,
                onCellClick = { index ->
                    val column = index - 1
                    if (column == sortColumn) ascending = !ascending else { sortColumn = column; ascending = true }
                },
            )
            HorizontalDivider()
            LazyColumn(Modifier.fillMaxSize()) {
                itemsIndexed(visibleRows) { index, row ->
                    TableRow(cells = listOf((index + 1).toString()) + row, header = false, onCellClick = {})
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                }
            }
        }
    }
}

@Composable
private fun TableRow(cells: List<String>, header: Boolean, onCellClick: (Int) -> Unit) {
    Row(
        modifier = Modifier
            .background(if (header) MaterialTheme.colorScheme.surfaceVariant else MaterialTheme.colorScheme.surface)
            .padding(horizontal = 8.dp, vertical = 6.dp),
    ) {
        cells.forEachIndexed { index, cell ->
            Text(
                text = cell,
                modifier = Modifier
                    .width(if (index == 0) 56.dp else 144.dp)
                    .padding(end = 10.dp)
                    .let { base -> if (header && index > 0) base.clickable { onCellClick(index) } else base },
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.bodySmall.copy(
                    fontFamily = if (header) FontFamily.Default else FontFamily.Monospace,
                    fontWeight = if (header) FontWeight.SemiBold else FontWeight.Normal,
                ),
            )
        }
    }
}

private fun compareCells(left: String, right: String): Int {
    val leftNumber = left.toDoubleOrNull()
    val rightNumber = right.toDoubleOrNull()
    return if (leftNumber != null && rightNumber != null) leftNumber.compareTo(rightNumber)
    else left.compareTo(right, ignoreCase = true)
}
