package com.rstudio.mobile.components

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.TextButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.rstudio.mobile.util.AnsiParser

@Composable
fun ConsoleView(
    console: String,
    lastValueSummary: String,
    isRunning: Boolean,
    status: String,
    onClear: () -> Unit,
    onCancel: () -> Unit,
) {
    val consoleLines = console.lines()
    val listState = rememberLazyListState()

    LaunchedEffect(consoleLines.size) {
        listState.animateScrollToItem(consoleLines.size - 1)
    }

    Column(Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            AssistChip(onClick = {}, label = { Text(status) })
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (isRunning) {
                    Button(onClick = onCancel) { Text("Cancel") }
                }
                TextButton(onClick = onClear) { Text("Clear") }
            }
        }
        Card(Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 4.dp)) {
            Text(
                text = lastValueSummary,
                modifier = Modifier.padding(12.dp),
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        HorizontalDivider()
        Box(Modifier.fillMaxSize()) {
            LazyColumn(
                state = listState,
                modifier = Modifier.fillMaxSize().padding(8.dp)
            ) {
                items(consoleLines) { line ->
                    Text(
                        text = AnsiParser.parse(line),
                        style = MaterialTheme.typography.bodyMedium.copy(
                            fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                            fontSize = 13.sp,
                            lineHeight = 18.sp
                        )
                    )
                }
            }
        }
    }
}
