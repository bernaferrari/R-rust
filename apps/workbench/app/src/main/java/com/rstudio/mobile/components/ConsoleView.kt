package com.rstudio.mobile.components

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.automirrored.filled.Send
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.rstudio.mobile.R
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
    var historyIndex by remember(history) { mutableIntStateOf(history.size) }
    var draft by remember { mutableStateOf("") }
    val actionModifier = Modifier.sizeIn(minWidth = 48.dp, minHeight = 48.dp)
    val outputLabel = stringResource(R.string.console_output)
    val historyPosition = if (historyIndex == history.size) {
        stringResource(R.string.console_history_draft)
    } else {
        stringResource(R.string.console_history_position, historyIndex + 1, history.size)
    }

    fun previousCommand() {
        if (isRunning || history.isEmpty()) return
        val index = historyIndex.coerceIn(0, history.size)
        if (index == 0) return
        if (index == history.size) draft = input
        historyIndex = index - 1
        input = history[historyIndex]
    }

    fun nextCommand() {
        if (isRunning || history.isEmpty()) return
        val index = historyIndex.coerceIn(0, history.size)
        if (index == history.size) return
        historyIndex = index + 1
        input = history.getOrElse(historyIndex) { draft }
    }

    fun submit() {
        val command = input.trim()
        if (command.isNotEmpty() && !isRunning) {
            onEvaluate(command)
            input = ""
            draft = ""
            historyIndex = history.size
        }
    }

    LaunchedEffect(consoleLines.size) {
        if (consoleLines.isNotEmpty()) listState.scrollToItem(consoleLines.lastIndex)
    }

    Column(Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 6.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                status,
                modifier = Modifier.weight(1f).padding(end = 8.dp).semantics { liveRegion = LiveRegionMode.Polite },
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (isRunning) Button(onClick = onCancel, modifier = actionModifier) { Text(stringResource(R.string.console_stop)) }
                TextButton(onClick = onClear, modifier = actionModifier) { Text(stringResource(R.string.console_clear)) }
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
                    modifier = Modifier.padding(10.dp).semantics { liveRegion = LiveRegionMode.Polite },
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
        HorizontalDivider()
        LazyColumn(
            state = listState,
            modifier = Modifier.weight(1f).fillMaxWidth()
                .semantics { contentDescription = outputLabel }
                .padding(horizontal = 10.dp, vertical = 6.dp),
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
        if (history.isNotEmpty()) {
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 10.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                TextButton(
                    onClick = ::previousCommand,
                    enabled = !isRunning && historyIndex > 0,
                    modifier = actionModifier.semantics { stateDescription = historyPosition },
                ) {
                    Icon(Icons.Default.KeyboardArrowUp, contentDescription = null)
                    Text(stringResource(R.string.console_previous_command))
                }
                TextButton(
                    onClick = ::nextCommand,
                    enabled = !isRunning && historyIndex < history.size,
                    modifier = actionModifier.semantics { stateDescription = historyPosition },
                ) {
                    Icon(Icons.Default.KeyboardArrowDown, contentDescription = null)
                    Text(stringResource(R.string.console_next_command))
                }
            }
        }
        OutlinedTextField(
            value = input,
            onValueChange = { input = it },
            modifier = Modifier
                .fillMaxWidth()
                .padding(10.dp)
                .onPreviewKeyEvent { event ->
                    if (event.type != KeyEventType.KeyDown || history.isEmpty() || isRunning) return@onPreviewKeyEvent false
                    when (event.key) {
                        Key.DirectionUp -> {
                            previousCommand()
                            true
                        }
                        Key.DirectionDown -> {
                            nextCommand()
                            true
                        }
                        else -> false
                    }
                },
            enabled = !isRunning,
            label = { Text(stringResource(R.string.console_input_label)) },
            placeholder = { Text(stringResource(R.string.console_input_hint)) },
            textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace, fontSize = 16.sp),
            singleLine = true,
            keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(imeAction = ImeAction.Send),
            keyboardActions = KeyboardActions(onSend = { submit() }),
            trailingIcon = {
                IconButton(onClick = ::submit, enabled = input.isNotBlank() && !isRunning, modifier = actionModifier) {
                    Icon(Icons.AutoMirrored.Filled.Send, contentDescription = stringResource(R.string.console_run_command))
                }
            },
        )
    }
}
