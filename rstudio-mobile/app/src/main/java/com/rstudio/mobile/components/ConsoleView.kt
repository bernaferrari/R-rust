package com.rstudio.mobile.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.rstudio.mobile.util.AnsiParser
import com.rstudio.mobile.util.AnsiPalette

@Composable
fun ConsoleView(
    console: String,
    history: List<String>,
    lastValueSummary: String,
    errorMessage: String?,
    isRunning: Boolean,
    status: String,
    onEvaluate: (String) -> Unit,
    onClear: () -> Unit,
    onCancel: () -> Unit,
) {
    val isDarkConsole = MaterialTheme.colorScheme.background.luminance() < 0.5f
    val ansiPalette = if (isDarkConsole) AnsiPalette.dark else AnsiPalette.light
    val consoleLines = remember(console, ansiPalette) { AnsiParser.parseLines(console, ansiPalette) }
    val listState = rememberLazyListState()
    var input by remember { mutableStateOf("") }
    var historyIndex by remember(history.size) { mutableIntStateOf(history.size) }

    fun submit() {
        val command = input.trim()
        if (command.isNotEmpty() && !isRunning) {
            onEvaluate(command)
            input = ""
            historyIndex = history.size + 1
        }
    }

    LaunchedEffect(consoleLines.size) {
        if (consoleLines.isNotEmpty()) listState.scrollToItem(consoleLines.lastIndex)
    }

    Column(Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 6.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            AssistChip(onClick = {}, label = { Text(status) })
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (isRunning) Button(onClick = onCancel) { Text("Stop") }
                TextButton(onClick = onClear) { Text("Clear") }
            }
        }
        if (errorMessage != null || lastValueSummary != "No result yet") {
            Card(
                Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 2.dp),
                colors = CardDefaults.cardColors(
                    containerColor = if (errorMessage == null) MaterialTheme.colorScheme.surfaceVariant
                    else MaterialTheme.colorScheme.errorContainer,
                ),
            ) {
                Text(
                    text = errorMessage ?: lastValueSummary,
                    modifier = Modifier.padding(10.dp),
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
        HorizontalDivider()
        LazyColumn(
            state = listState,
            modifier = Modifier.weight(1f).fillMaxWidth().padding(horizontal = 10.dp, vertical = 6.dp),
        ) {
            items(consoleLines) { line ->
                Text(
                    text = line,
                    style = MaterialTheme.typography.bodyMedium.copy(
                        fontFamily = FontFamily.Monospace,
                        fontSize = 13.sp,
                        lineHeight = 18.sp,
                    ),
                )
            }
        }
        OutlinedTextField(
            value = input,
            onValueChange = { input = it },
            modifier = Modifier
                .fillMaxWidth()
                .padding(10.dp)
                .onPreviewKeyEvent { event ->
                    if (event.type != KeyEventType.KeyDown || history.isEmpty()) return@onPreviewKeyEvent false
                    when (event.key) {
                        Key.DirectionUp -> {
                            historyIndex = (historyIndex - 1).coerceAtLeast(0)
                            input = history[historyIndex]
                            true
                        }
                        Key.DirectionDown -> {
                            historyIndex = (historyIndex + 1).coerceAtMost(history.size)
                            input = history.getOrElse(historyIndex) { "" }
                            true
                        }
                        else -> false
                    }
                },
            enabled = !isRunning,
            label = { Text("R console") },
            placeholder = { Text("Type an R expression") },
            textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace, fontSize = 16.sp),
            singleLine = true,
            keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(imeAction = ImeAction.Send),
            keyboardActions = KeyboardActions(onSend = { submit() }),
            trailingIcon = {
                IconButton(onClick = ::submit, enabled = input.isNotBlank() && !isRunning) {
                    Icon(Icons.AutoMirrored.Filled.Send, contentDescription = "Run console command")
                }
            },
        )
    }
}
